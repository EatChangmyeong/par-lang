use lsp_types::request::Request;
use serde::{Deserialize, Serialize};

pub enum GetBuiltinFileContent {}

impl Request for GetBuiltinFileContent {
    type Params = GetBuiltinFileContentParams;
    type Result = String;
    const METHOD: &'static str = "par-lang/getBuiltinFileContent";
}

#[derive(Serialize, Deserialize)]
pub struct GetBuiltinFileContentParams {
    pub builtin_path: String,
}
