/// Common error/result types for the metadata gateway loop.

pub(crate) type MetaError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub(crate) type MetaResult<T> = Result<T, MetaError>;
