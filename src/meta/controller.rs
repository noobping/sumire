use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{atomic::AtomicU64, Arc};
use std::thread;

use crate::station::Station;

use super::gateway::run_meta_loop;
use super::track::TrackInfo;

#[derive(Debug)]
pub(crate) enum Control {
    Stop,
    Pause,
    Resume,
}

#[derive(Debug)]
enum State {
    Stopped,
    Running { tx: mpsc::Sender<Control> },
}

#[derive(Debug)]
struct Inner {
    station: Station,
    state: State,
    sender: mpsc::Sender<TrackInfo>,
    lag_ms: Arc<AtomicU64>,
    ui_sched_id: Arc<AtomicU64>,
}

#[derive(Debug)]
pub struct Meta {
    inner: RefCell<Inner>,
}

impl Meta {
    pub fn new(
        station: Station,
        sender: mpsc::Sender<TrackInfo>,
        lag_ms: Arc<AtomicU64>,
    ) -> Rc<Self> {
        Rc::new(Self {
            inner: RefCell::new(Inner {
                station,
                state: State::Stopped,
                sender,
                lag_ms,
                ui_sched_id: Arc::new(AtomicU64::new(0)),
            }),
        })
    }

    pub fn set_station(&self, station: Station) {
        let mut inner = self.inner.borrow_mut();
        let was_running = matches!(inner.state, State::Running { .. });
        if was_running {
            Self::stop_inner(&mut inner);
        }
        inner.station = station;
        if was_running {
            Self::start_inner(&mut inner);
        }
    }

    pub fn start(&self) {
        let tx_opt = {
            let inner = self.inner.borrow();
            match &inner.state {
                State::Running { tx } => Some(tx.clone()),
                State::Stopped => None,
            }
        };
        if let Some(tx) = tx_opt {
            let _ = tx.send(Control::Resume);
            return;
        }
        // stopped: actually start thread
        let mut inner = self.inner.borrow_mut();
        Self::start_inner(&mut inner);
    }

    pub fn pause(&self) {
        let inner = self.inner.borrow();
        if let State::Running { tx } = &inner.state {
            let _ = tx.send(Control::Pause);
        }
    }

    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }

    fn start_inner(inner: &mut Inner) {
        match inner.state {
            State::Running { .. } => return,
            State::Stopped => {
                let (tx, rx) = mpsc::channel::<Control>();
                let station = inner.station;
                let sender = inner.sender.clone();
                let lag_ms = inner.lag_ms.clone();
                let ui_sched_id = inner.ui_sched_id.clone();

                inner.state = State::Running { tx: tx.clone() };

                thread::spawn(move || {
                    if let Err(err) = run_meta_loop(station, sender, rx, lag_ms, ui_sched_id) {
                        eprintln!("Gateway error in metadata loop: {err}");
                    }
                });
            }
        }
    }

    fn stop_inner(inner: &mut Inner) {
        if let State::Running { tx } = &inner.state {
            let _ = tx.send(Control::Stop);
        }
        inner.state = State::Stopped;
    }
}

impl Drop for Meta {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }
}
