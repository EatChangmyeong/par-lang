use std::{future::Future, pin::Pin, sync::Arc};

use arcstr::ArcStr;
use indexmap::IndexMap;

use crate::{
    icombs::readback::Handle,
    location::{Point, Span},
    par::parse::parse_module,
};

use super::{
    language::{CompileError, GlobalName, LocalName},
    parse::SyntaxError,
    process,
    types::{Context, Type, TypeDefs, TypeError},
};

#[derive(Clone, Debug)]
pub struct Module<Expr> {
    pub type_defs: Vec<TypeDef>,
    pub declarations: Vec<Declaration>,
    pub definitions: Vec<Definition<Expr>>,
}

#[derive(Debug, Clone)]
pub struct CheckedModule {
    pub type_defs: TypeDefs,
    pub declarations: IndexMap<GlobalName, Declaration>,
    pub definitions: IndexMap<GlobalName, Definition<Arc<process::Expression<Type>>>>,
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    pub span: Span,
    pub name: GlobalName,
    pub params: Vec<LocalName>,
    pub typ: Type,
}

#[derive(Clone, Debug)]
pub struct Declaration {
    pub span: Span,
    pub name: GlobalName,
    pub typ: Type,
}

#[derive(Clone, Debug)]
pub struct Definition<Expr> {
    pub span: Span,
    pub name: GlobalName,
    pub expression: Expr,
}

impl TypeDef {
    pub fn external(name: &'static str, params: &[&'static str], typ: Type) -> Self {
        Self {
            span: Default::default(),
            name: GlobalName::external(None, name),
            params: params
                .into_iter()
                .map(|&var| LocalName {
                    span: Default::default(),
                    string: ArcStr::from(var),
                })
                .collect(),
            typ,
        }
    }
}

impl Definition<Arc<process::Expression<()>>> {
    pub fn external(
        name: &'static str,
        typ: Type,
        f: fn(Handle) -> Pin<Box<dyn Send + Future<Output = ()>>>,
    ) -> Self {
        Self {
            span: Default::default(),
            name: GlobalName::external(None, name),
            expression: Arc::new(process::Expression::External(typ, f, ())),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParseAndCompileError {
    Parse(SyntaxError),
    Compile(CompileError),
}

impl From<SyntaxError> for ParseAndCompileError {
    fn from(value: SyntaxError) -> Self {
        Self::Parse(value)
    }
}

impl From<CompileError> for ParseAndCompileError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}

impl Module<Arc<process::Expression<()>>> {
    pub fn parse_and_compile(source: &str) -> Result<Self, ParseAndCompileError> {
        let parsed = parse_module(source)?;

        let compiled_definitions = parsed
            .definitions
            .into_iter()
            .map(
                |Definition {
                     span,
                     name,
                     expression,
                 }| {
                    expression.compile().map(|compiled| Definition {
                        span,
                        name,
                        expression: compiled.optimize().fix_captures(&IndexMap::new()).0,
                    })
                },
            )
            .collect::<Result<_, _>>()?;

        Ok(Module {
            type_defs: parsed.type_defs,
            declarations: parsed.declarations,
            definitions: compiled_definitions,
        })
    }

    pub fn import(&mut self, module_name: Option<&str>, module: Self) {
        let mut module = module;
        module.qualify(module_name);
        self.type_defs.append(&mut module.type_defs);
        self.declarations.append(&mut module.declarations);
        self.definitions.append(&mut module.definitions);
    }

    fn qualify(&mut self, module: Option<&str>) {
        for TypeDef {
            span: _,
            name,
            params: _,
            typ,
        } in &mut self.type_defs
        {
            name.qualify(module);
            typ.qualify(module);
        }
        for Declaration { span: _, name, typ } in &mut self.declarations {
            name.qualify(module);
            typ.qualify(module);
        }
        for Definition {
            span: _,
            name,
            expression,
        } in &mut self.definitions
        {
            name.qualify(module);
            *expression = expression.clone().qualify(module);
        }
    }

    pub fn type_check(&self) -> Result<CheckedModule, TypeError> {
        let type_defs = TypeDefs::new_with_validation(
            self.type_defs
                .iter()
                .map(|d| (&d.span, &d.name, &d.params, &d.typ)),
        )?;

        let mut unchecked_definitions = IndexMap::new();
        for Definition {
            span,
            name,
            expression,
        } in &self.definitions
        {
            if let Some((span1, _)) =
                unchecked_definitions.insert(name.clone(), (span.clone(), expression.clone()))
            {
                return Err(TypeError::NameAlreadyDefined(
                    span.clone(),
                    span1.clone(),
                    name.clone(),
                ));
            }
        }

        let mut declarations = IndexMap::new();
        for Declaration { span, name, typ } in &self.declarations {
            if !unchecked_definitions.contains_key(name) {
                return Err(TypeError::DeclaredButNotDefined(span.clone(), name.clone()));
            }
            if let Some((span1, _)) = declarations.insert(name.clone(), (span.clone(), typ.clone()))
            {
                return Err(TypeError::NameAlreadyDeclared(
                    span.clone(),
                    span1,
                    name.clone(),
                ));
            }
        }

        let names_to_check = unchecked_definitions
            .iter()
            .map(|(name, (span, _))| (span.clone(), name.clone()))
            .collect::<Vec<_>>();

        let mut context = Context::new(type_defs, declarations, unchecked_definitions);
        for (span, name) in names_to_check {
            context.check_definition(&span, &name)?;
        }

        Ok(CheckedModule {
            type_defs: context.get_type_defs().clone(),
            declarations: context
                .get_declarations()
                .into_iter()
                .map(|(name, (span, typ))| (name.clone(), Declaration { span, name, typ }))
                .collect(),
            definitions: context
                .get_checked_definitions()
                .into_iter()
                .map(|(name, (span, expression))| {
                    (
                        name.clone(),
                        Definition {
                            span,
                            name,
                            expression,
                        },
                    )
                })
                .collect(),
        })
    }
}

impl<Expr> Default for Module<Expr> {
    fn default() -> Self {
        Self {
            type_defs: Vec::new(),
            declarations: Vec::new(),
            definitions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NameWithType(pub Option<String>, pub Type);

pub struct TypeOnHover {
    sorted_pairs: Vec<((Point, Point), NameWithType)>,
}

impl TypeOnHover {
    pub fn new(program: &CheckedModule) -> Self {
        let mut pairs = Vec::new();

        for (name, (_, _, typ)) in program.type_defs.globals.iter() {
            if let Some((start, end)) = name.span.points() {
                pairs.push((
                    (start, end),
                    NameWithType(Some(format!("{}", name)), typ.clone()),
                ));
            }
            typ.types_at_spans(&program.type_defs, &mut |span, name, typ| {
                if let Some((start, end)) = span.points() {
                    pairs.push(((start, end), NameWithType(name, typ)))
                }
            });
        }

        for (name, declaration) in &program.declarations {
            if let Some((start, end)) = name.span.points() {
                pairs.push((
                    (start, end),
                    NameWithType(Some(format!("{}", name)), declaration.typ.clone()),
                ));
            }
            declaration
                .typ
                .types_at_spans(&program.type_defs, &mut |span, name, typ| {
                    if let Some((start, end)) = span.points() {
                        pairs.push(((start, end), NameWithType(name, typ)))
                    }
                });
        }

        for (name, definition) in &program.definitions {
            if let Some((start, end)) = name.span.points() {
                pairs.push((
                    (start, end),
                    NameWithType(Some(format!("{}", name)), definition.expression.get_type()),
                ));
            }
            definition
                .expression
                .types_at_spans(&program.type_defs, &mut |span, name, typ| {
                    if let Some((start, end)) = span.points() {
                        pairs.push(((start, end), NameWithType(name, typ)))
                    }
                });
        }

        pairs.sort_by_key(|((start, _), _)| start.offset);
        pairs.dedup_by_key(|((start, _), _)| start.offset);

        Self {
            sorted_pairs: pairs,
        }
    }
}

impl TypeOnHover {
    pub fn query(&self, row: usize, column: usize) -> Option<NameWithType> {
        if self.sorted_pairs.is_empty() {
            return None;
        }

        // find index with the greatest start that is <= than (row, column)
        let (mut lo, mut hi) = (0, self.sorted_pairs.len());
        while lo + 1 < hi {
            let mi = (lo + hi) / 2;
            let ((mp, _), _) = self.sorted_pairs[mi];
            if mp.row < row || (mp.row == row && mp.column <= column) {
                lo = mi;
            } else {
                hi = mi;
            }
        }

        let ((start, end), typ) = &self.sorted_pairs[lo];

        // check if queried (row, column) is in the found span
        if row < start.row || (row == start.row && column < start.column) {
            return None;
        }
        if end.row < row || (end.row == row && end.column < column) {
            return None;
        }

        // found a good span
        Some(typ.clone())
    }
}
