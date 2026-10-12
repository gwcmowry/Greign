//! RAW input contracts and test-corpus metadata.

pub mod corpus;
pub mod decoder;
pub mod manifest;
pub mod rawler_backend;

pub use decoder::{
    ActiveArea, DecodeError, DecoderCapabilities, EmbeddedPreview, Mosaic, Orientation,
    PreviewEncoding, RawDecoder, RawMetadata,
};
pub use rawler_backend::RawlerDecoder;
