//! Receipt composition and printer command generation. No transport or JavaScript.

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

mod compose;
mod images;
pub mod model;

#[derive(Debug, Default)]
pub struct Outcome {
    pub escpos: Vec<u8>,
    pub tickets: usize,
    pub diagnostics: Vec<String>,
    pub profile: model::ResolvedProfile,
}

pub struct Image {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn render(json: &[u8]) -> Result<Outcome, Error> {
    render_with_images(json, &[])
}

pub fn render_with_images(json: &[u8], images: &[Image]) -> Result<Outcome, Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    let receipt: model::Receipt =
        serde_path_to_error::deserialize(&mut deserializer).map_err(|e| {
            Error(format!(
                "receipt is not valid JSON at {}: {}",
                e.path(),
                e.inner()
            ))
        })?;
    deserializer
        .end()
        .map_err(|e| Error(format!("receipt is not valid JSON: {e}")))?;
    compose::render(&receipt, images)
}
