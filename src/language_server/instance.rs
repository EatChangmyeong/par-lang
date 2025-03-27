use std::collections::HashMap;
use lsp_types::{self as lsp, DocumentSymbolResponse, Uri};
use crate::par::language::{Declaration, Definition, Internal, Name, TypeDef};
use crate::location::Span;
use crate::par::types::TypeError;
use crate::playground::Compiled;
use super::io::IO;

#[derive(Debug, Clone)]
pub enum CompileError {
    Compile(crate::playground::Error),
    Types(TypeError<Internal<Name>>),
}
pub type CompileResult = Result<Compiled, CompileError>;

pub struct Instance {
    uri: Uri,
    dirty: bool,
    compiled: Option<CompileResult>,
    io: IO,
}

impl Instance {
    pub fn new(uri: Uri, io: IO) -> Instance {
        Self {
            uri,
            dirty: true,
            compiled: None,
            io
        }
    }

    pub fn handle_hover(&self, params: &lsp::HoverParams) -> Option<lsp::Hover> {
        tracing::debug!("Handling hover request with params: {:?}", params);

        let pos = params.text_document_position_params.position;

        let payload = match &self.compiled {
            Some(Ok(compiled)) => {
                let mut message: Option<String> = Some(format!("{}:{}", pos.line, pos.character));

                let mut inside_item = false;

                for TypeDef { span, name, .. } in &compiled.program.type_defs {
                    if !is_inside(pos, span) {
                        continue;
                    }
                    inside_item = true;
                    message = Some(format!("Type: {}", name.to_string()));
                    break;
                }

                if !inside_item {
                    for Declaration { span, name, typ } in &compiled.program.declarations {
                        if !is_inside(pos, span) {
                            continue;
                        }
                        inside_item = true;
                        let mut msg = format!("Declaration: {}: ", name.to_string());
                        let indent = msg.len();
                        typ.pretty(&mut msg, indent + 1).unwrap();
                        message = Some(msg);
                        break;
                    }
                }

                if !inside_item {
                    for Definition { span, name, expression } in &compiled.program.definitions {
                        if !is_inside(pos, span) {
                            continue;
                        }
                        inside_item = true;
                        let mut msg = format!("Definition: {}: ", name.to_string());
                        let indent = msg.len();
                        expression.pretty(&mut msg, indent + 1).unwrap();
                        message = Some(msg);
                        break;
                    }
                }

                if let Some(message) = message {
                    message
                } else {
                    format!("Compiled:\n{}", compiled.pretty.clone())
                }
            },
            Some(Err(e)) => format!("Compiled error: {:?}", e),
            None => "Not compiled".to_string(),
        };

        let hover = lsp::Hover {
            contents: lsp::HoverContents::Scalar(
                lsp::MarkedString::String(payload)
            ),
            range: None,
        };
        Some(hover)
    }

    pub fn handle_document_symbols(&self, params: &lsp::DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        tracing::info!("Handling symbols request with params: {:?}", params);

        let Some(Ok(compiled)) = &self.compiled else {
            return None;
        };

        let mut symbols = HashMap::new();

        for TypeDef { span, name, .. } in &compiled.program.type_defs {
            symbols.insert(name, lsp::DocumentSymbol {
                name: name.to_string(),
                detail: None,
                kind: lsp::SymbolKind::INTERFACE, // fits best?
                tags: None,
                deprecated: None, // must be specified
                range: span.into(),
                selection_range: name.span().unwrap().into(), // always in code
                children: None,
            });
        }

        for Declaration { span, name, typ } in &compiled.program.declarations {
            let mut detail = String::new();
            typ.pretty(&mut detail, 0).unwrap();
            symbols.insert(name, lsp::DocumentSymbol {
                name: name.to_string(),
                detail: Some(detail),
                kind: lsp::SymbolKind::FUNCTION, // something else for non-functions?
                tags: None,
                deprecated: None, // must be specified
                range: span.into(),
                selection_range: name.span().unwrap().into(), // always in code
                children: None,
            });
        }

        for Definition { span, name, .. } in &compiled.program.definitions {
            let range = span.into();
            let selection_range = name.span().unwrap().into(); // always in code
            symbols.entry(name)
                .and_modify(|symbol| {
                    symbol.range = range;
                    symbol.selection_range = selection_range;
                })
                .or_insert(lsp::DocumentSymbol {
                    name: name.to_string(),
                    detail: None,
                    kind: lsp::SymbolKind::FUNCTION, // something else for non-functions?
                    tags: None,
                    deprecated: None, // must be specified
                    range,
                    selection_range,
                    children: None,
                });
        }

        Some(DocumentSymbolResponse::Nested(
            symbols.into_iter().map(|(_, v)| v).collect()
        ))
    }

    pub fn compile(&mut self) -> Result<(), CompileError> {
        tracing::info!("Compiling: {:?}", self.uri);
        if !self.dirty {
            tracing::info!("No changes");
            tracing::debug!("No changes to compile");
            return Ok(());
        }
        let code = self.io.read(&self.uri);

        // todo: progress reporting
        let mut compiled = stacker::grow(32 * 1024 * 1024, || {
            Compiled::from_string(&code.unwrap())
        }).map_err(|err| CompileError::Compile(err));
        match compiled {
            Ok(Compiled { checked: Err(err), .. }) => {
                compiled = Err(CompileError::Types(err));
            }
            _ => {}
        }

        let result = match &compiled {
            Ok(_) => {
                self.dirty = false;
                tracing::info!("Compilation successful");
                Ok(())
            }
            Err(err) => {
                tracing::info!("Compilation failed");
                Err(err.clone())
            }
        };

        self.compiled = Some(compiled);

        result
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

fn is_inside(pos: lsp::Position, span: &Span) -> bool {
    let pos_row = pos.line as usize;
    let pos_column = pos.character as usize;

    let start = span.start;
    let end = span.end;

    !(pos_row < start.row || pos_row > end.row)
        && !(pos_row == start.row && pos_column < start.column)
        && !(pos_row == end.row && pos_column > end.column)
}