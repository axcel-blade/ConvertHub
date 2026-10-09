//! User-facing errors. The backend never builds English sentences: it returns a
//! translation key plus variables, and the frontend renders them from the
//! locale files.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct UiMsg {
    pub key: String,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
}

impl UiMsg {
    pub fn new(key: &str) -> Self {
        Self { key: key.to_string(), vars: BTreeMap::new() }
    }
    pub fn var(mut self, k: &str, v: impl ToString) -> Self {
        self.vars.insert(k.to_string(), v.to_string());
        self
    }
}

/// Serialized transparently as `{key, vars}` across the IPC boundary.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct UiError(pub UiMsg);

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.key)?;
        for (k, v) in &self.0.vars {
            write!(f, " {k}={v}")?;
        }
        Ok(())
    }
}

impl std::error::Error for UiError {}

pub type R<T> = Result<T, UiError>;

/// `ui!(<translation key>, "var" => value, ...)` builds a [`UiError`].
#[macro_export]
macro_rules! ui {
    ($key:expr $(, $k:expr => $v:expr)* $(,)?) => {
        $crate::error::UiError($crate::error::UiMsg::new($key)$(.var($k, $v))*)
    };
}

pub fn generic(detail: impl ToString) -> UiError {
    UiError(UiMsg::new("errors.generic").var("detail", detail))
}

macro_rules! impl_generic_from {
    ($($t:ty),*) => {$(
        impl From<$t> for UiError {
            fn from(e: $t) -> Self { generic(e) }
        }
    )*};
}

impl_generic_from!(
    std::io::Error,
    serde_json::Error,
    image::ImageError,
    zip::result::ZipError,
    lopdf::Error,
    rust_xlsxwriter::XlsxError,
    reqwest::Error,
    tauri::Error
);
