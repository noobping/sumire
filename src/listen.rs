use reqwest::blocking::Client;
use rodio::{buffer::SamplesBuffer, OutputStreamBuilder, Sink};
use rustfft::{num_complex::Complex32, FftPlanner};
use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;
use std::sync::atomic::AtomicU32;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Instant;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::http_source::HttpSource;
#[cfg(debug_assertions)]
use crate::log::now_string;
use crate::station::Station;

type DynError = Box<dyn Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, DynError>;

const N_BARS: usize = 48;

#[derive(Debug)]
enum Control {
    Stop,
    Pause,
    Resume,
}

#[derive(Debug)]
enum State {
    Stopped,
    Paused { tx: mpsc::Sender<Control> },
    Playing { tx: mpsc::Sender<Control> },
}

#[derive(Debug)]
struct Inner {
    station: Station,
    state: State,
}

#[derive(Debug)]
pub struct Listen {
    inner: RefCell<Inner>,
    lag_ms: Arc<AtomicU64>,
    pause_started: RefCell<Option<Instant>>,
    spectrum_bits: Arc<Vec<AtomicU32>>,
}

impl Listen {
    pub fn new(station: Station) -> Rc<Self> {
        Rc::new(Self {
            inner: RefCell::new(Inner {
                station,
                state: State::Stopped,
            }),
            lag_ms: Arc::new(AtomicU64::new(0)),
            pause_started: RefCell::new(None),
            spectrum_bits: Arc::new((0..N_BARS).map(|_| AtomicU32::new(0)).collect()),
        })
    }

    pub fn spectrum_bars(&self) -> Arc<Vec<AtomicU32>> {
        self.spectrum_bits.clone()
    }

    pub fn lag_ms(&self) -> Arc<AtomicU64> {
        self.lag_ms.clone()
    }

    pub fn get_station(&self) -> Station {
        self.inner.borrow_mut().station
    }

    pub fn set_station(&self, station: Station) {
        let mut inner = self.inner.borrow_mut();
        let was_playing_or_paused = matches!(inner.state, State::Playing { .. } | State::Paused { .. });
        if was_playing_or_paused {
            Self::stop_inner(&mut inner);
        }
        inner.station = station;
        if was_playing_or_paused {
            Self::start_inner(&mut inner, self.spectrum_bits.clone());
        }
    }

    pub fn start(&self) {
        if matches!(self.inner.borrow().state, State::Paused { .. }) {
            if let Some(t0) = self.pause_started.borrow_mut().take() {
                let add = t0.elapsed().as_millis() as u64;
                self.lag_ms.fetch_add(add, Ordering::Relaxed);
            }
        }
        let mut inner = self.inner.borrow_mut();
        Self::start_inner(&mut inner, self.spectrum_bits.clone());
    }

    pub fn pause(&self) {
        let mut inner = self.inner.borrow_mut();
        match &inner.state {
            State::Playing { tx } => {
                let _ = tx.send(Control::Pause);
                inner.state = State::Paused { tx: tx.clone() };
            }
            _ => {}
        }
        *self.pause_started.borrow_mut() = Some(Instant::now());
    }

    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }

    fn start_inner(inner: &mut Inner, spectrum_bits: Arc<Vec<AtomicU32>>) {
        match &inner.state {
            State::Playing { .. } => {
                // already playing
                return;
            }
            State::Paused { tx } => {
                let _ = tx.send(Control::Resume);
                inner.state = State::Playing { tx: tx.clone() };
                return;
            }
            State::Stopped => {
                let (tx, rx) = mpsc::channel::<Control>();
                let station = inner.station;

                inner.state = State::Playing { tx: tx.clone() };

                // detached worker thread; will exit on Stop or error
                thread::spawn(move || {
                    if let Err(err) = run_listenmoe_stream(station, rx, spectrum_bits) {
                        eprintln!("stream error: {err}");
                    }
                });
            }
        }
    }

    fn stop_inner(inner: &mut Inner) {
        if let State::Playing { tx } | State::Paused { tx } = &inner.state {
            let _ = tx.send(Control::Stop);
        }
        inner.state = State::Stopped;
    }
}

impl Drop for Listen {
    fn drop(&mut self) {
        // Best-effort cleanup
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }
}

fn run_listenmoe_stream(
    station: Station,
    rx: mpsc::Receiver<Control>,
    spectrum_bits: Arc<Vec<AtomicU32>>,
) -> Result<()> {
    let url = station.stream_url();
    #[cfg(debug_assertions)]
    println!("[{}] Connecting to {url}…", now_string());

    let client = Client::new();
    let platform = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    };
    let useragent = format!(
        "{}-v{}-{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        platform
    );
    let response = client.get(url).header("User-Agent", useragent).send()?;
    #[cfg(debug_assertions)]
    println!("[{}] HTTP status: {}", now_string(), response.status());
    if !response.status().is_success() {
        return Err(format!("HTTP status {}", response.status()).into());
    }

    let http_source = HttpSource { inner: response };
    let mss = MediaSourceStream::new(Box::new(http_source), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    let probed =
        symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "no supported audio tracks".to_string())?;

    let mut track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

    let stream = OutputStreamBuilder::open_default_stream()?;
    let sink = Sink::connect_new(&stream.mixer());

    #[cfg(debug_assertions)]
    println!("[{}] Started decoding + playback.", now_string());

    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut paused = false;

    // FFT state
    const FFT_SIZE: usize = 1024;
    const HOP: usize = 512;

    let mut fft_planner = FftPlanner::<f32>::new();
    let fft = fft_planner.plan_fft_forward(FFT_SIZE);
    let window = hann_window(FFT_SIZE);

    let mut mono_ring: Vec<f32> = Vec::with_capacity(FFT_SIZE * 4);
    let mut fft_in: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
    let mut mags: Vec<f32> = vec![0.0; FFT_SIZE / 2];
    let mut bars: Vec<f32> = vec![0.0; spectrum_bits.len()];
    let mut bars_smooth: Vec<f32> = vec![0.0; spectrum_bits.len()];
    let mut bar_peak: Vec<f32> = vec![0.0; spectrum_bits.len()];

    let peak_attack = 0.35f32;   // how fast peak rises (0..1)  (bigger = faster)
    let peak_release = 0.995f32; // how fast peak falls (0..1) (closer to 1 = slower)
    let sensitivity = 1.25f32;   // overall gain (bigger = taller bars)
    let curve = 0.75f32;         // <1 boosts quiet, >1 compresses

    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Control::Stop => {
                    #[cfg(debug_assertions)]
                    println!("[{}] Stop requested, shutting down stream.", now_string());
                    sink.stop();
                    return Ok(());
                }
                Control::Pause => {
                    if !paused {
                        #[cfg(debug_assertions)]
                        println!("[{}] Pausing playback.", now_string());
                        paused = true;
                        sink.pause();
                    }
                }
                Control::Resume => {
                    if paused {
                        #[cfg(debug_assertions)]
                        println!("[{}] Resuming playback.", now_string());
                        paused = false;
                        sink.play();
                    }
                }
            }
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                #[cfg(debug_assertions)]
                println!("[{}] Stream reset, reconfiguring decoder…", now_string());
                let new_track = format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
                    .ok_or_else(|| "no supported audio tracks after reset".to_string())?;

                track_id = new_track.id;
                decoder = symphonia::default::get_codecs()
                    .make(&new_track.codec_params, &decoder_opts)?;

                sample_buf = None;
                continue;
            }
            Err(err) => {
                return Err(format!("Error reading packet: {err:?}").into());
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                #[cfg(debug_assertions)]
                println!(
                    "[{}] Decoder reset required, rebuilding decoder…",
                    now_string()
                );
                let new_track = format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
                    .ok_or_else(|| "no supported audio tracks after decoder reset".to_string())?;

                track_id = new_track.id;
                decoder = symphonia::default::get_codecs()
                    .make(&new_track.codec_params, &decoder_opts)?;
                sample_buf = None;
                continue;
            }
            Err(err) => {
                return Err(format!("Fatal decode error: {err:?}").into());
            }
        };

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;

            channels = spec.channels.count() as u16;
            sample_rate = spec.rate;

            sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
        }

        let buf = sample_buf.as_mut().expect("sample_buf just initialized");
        buf.copy_interleaved_ref(decoded);
        let samples = buf.samples().to_owned();

        // Build mono ring buffer for FFT (downmix)
        let ch = channels as usize;
        if ch > 0 {
            let frames = samples.len() / ch;
            mono_ring.reserve(frames);

            for f in 0..frames {
                let mut acc = 0.0f32;
                for c in 0..ch {
                    acc += samples[f * ch + c];
                }
                mono_ring.push(acc / (ch as f32));
            }
        }

        // Run FFT every HOP samples (50% overlap)
        while mono_ring.len() >= FFT_SIZE {
            // window into complex input
            for i in 0..FFT_SIZE {
                let x = mono_ring[i] * window[i];
                fft_in[i] = Complex32::new(x, 0.0);
            }

            // execute FFT in-place
            fft.process(&mut fft_in);

            // magnitudes
            for i in 0..(FFT_SIZE / 2) {
                let c = fft_in[i];
                mags[i] = (c.re * c.re + c.im * c.im).sqrt();
            }

            // map bins -> bars (0..1)
            bins_to_bars(&mags, sample_rate, &mut bars);

            for i in 0..bars.len() {
                let v = bars[i].max(1e-12);

                // Initialize peak the first time we see data
                if bar_peak[i] == 0.0 {
                    bar_peak[i] = v;
                }

                // Peak follower: fast attack, slow release
                if v > bar_peak[i] {
                    bar_peak[i] = bar_peak[i] + (v - bar_peak[i]) * peak_attack;
                } else {
                    bar_peak[i] *= peak_release;
                    if bar_peak[i] < 1e-12 {
                        bar_peak[i] = 1e-12;
                    }
                }

                // Normalize by peak (floor included)
                let mut x = v / (bar_peak[i] + 1e-12);

                // Apply gain + curve
                x = (x * sensitivity).clamp(0.0, 1.0);
                x = x.powf(curve);

                bars[i] = x;
            }

            // smooth bars (EMA) AFTER AGC normalization
            for i in 0..bars.len() {
                bars_smooth[i] = bars_smooth[i] * 0.80 + bars[i] * 0.20;
            }

            // publish to atomics (publish smoothed bars)
            for (i, v) in bars_smooth.iter().enumerate() {
                spectrum_bits[i].store(v.to_bits(), Ordering::Relaxed);
            }

            // slide ring by HOP
            let hop = HOP.min(mono_ring.len());
            mono_ring.drain(0..hop);
        }

        // Send audio to rodio
        let source = SamplesBuffer::new(channels, sample_rate, samples);
        sink.append(source);
    }
}

fn hann_window(n: usize) -> Vec<f32> {
    // Hann: 0.5 - 0.5*cos(2πk/(n-1))
    let denom = (n.saturating_sub(1)).max(1) as f32;
    (0..n)
        .map(|k| {
            let x = (2.0 * std::f32::consts::PI * k as f32) / denom;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

fn bins_to_bars(mags: &[f32], sample_rate: u32, bars_out: &mut [f32]) {
    let n_bins = mags.len().max(1);
    let sr = sample_rate as f32;

    let f_min = 60.0_f32;
    let f_max = 12_000.0_f32.min(sr * 0.5);

    let log_min = f_min.ln();
    let log_max = f_max.ln();
    let log_span = (log_max - log_min).max(1e-6);

    for v in bars_out.iter_mut() {
        *v = 0.0;
    }

    // For each bar, average magnitudes of bins in its freq range.
    for i in 0..bars_out.len() {
        let a = i as f32 / bars_out.len() as f32;
        let b = (i + 1) as f32 / bars_out.len() as f32;

        let f0 = (log_min + a * log_span).exp();
        let f1 = (log_min + b * log_span).exp();

        let bin0 = ((f0 / (sr * 0.5)) * (n_bins as f32)) as usize;
        let bin1 = ((f1 / (sr * 0.5)) * (n_bins as f32)) as usize;

        let lo = bin0.clamp(0, n_bins - 1);
        let hi = bin1.clamp(lo + 1, n_bins);

        let mut sum = 0.0f32;
        for &m in &mags[lo..hi] {
            sum += m;
        }
        let avg = sum / ((hi - lo) as f32);

        bars_out[i] = avg;
    }
}
