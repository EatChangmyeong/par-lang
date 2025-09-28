use super::{Type, TypeDefs, TypeError};
use crate::location::Span;
use crate::par::language::{GlobalName, LocalName};
use indexmap::IndexSet;

pub fn continue_<E, F>(typ: &Type, mut visit: F) -> Result<(), E>
where
    F: FnMut(&Type) -> Result<(), E>,
{
    match typ {
        Type::Name(_, _, args) => {
            for arg in args {
                visit(arg)?;
            }
        }
        Type::DualName(_, _, args) => {
            for arg in args {
                visit(arg)?;
            }
        }
        Type::Box(_, inner) | Type::DualBox(_, inner) => {
            visit(inner)?;
        }
        Type::Pair(_, left, right) | Type::Function(_, left, right) => {
            visit(left)?;
            visit(right)?;
        }
        Type::Either(_, branches) | Type::Choice(_, branches) => {
            for (_name, typ) in branches {
                visit(typ)?;
            }
        }
        Type::Recursive { body, .. } | Type::Iterative { body, .. } => {
            visit(body)?;
        }
        Type::Exists(_, _, body) | Type::Forall(_, _, body) => {
            visit(body)?;
        }
        Type::Primitive(..)
        | Type::DualPrimitive(..)
        | Type::Var(..)
        | Type::DualVar(..)
        | Type::Break(..)
        | Type::Continue(..)
        | Type::Self_(..)
        | Type::DualSelf(..) => {}
    }
    Ok(())
}

pub fn continue_mut<E, F>(typ: &mut Type, mut visit: F) -> Result<(), E>
where
    F: FnMut(&mut Type) -> Result<(), E>,
{
    match typ {
        Type::Name(_, _, args) => {
            for arg in args {
                visit(arg)?;
            }
        }
        Type::DualName(_, _, args) => {
            for arg in args {
                visit(arg)?;
            }
        }
        Type::Box(_, inner) | Type::DualBox(_, inner) => {
            visit(inner)?;
        }
        Type::Pair(_, left, right) | Type::Function(_, left, right) => {
            visit(left)?;
            visit(right)?;
        }
        Type::Either(_, branches) | Type::Choice(_, branches) => {
            for (_name, typ) in branches {
                visit(typ)?;
            }
        }
        Type::Recursive { body, .. } | Type::Iterative { body, .. } => {
            visit(body)?;
        }
        Type::Exists(_, _, body) | Type::Forall(_, _, body) => {
            visit(body)?;
        }
        Type::Primitive(..)
        | Type::DualPrimitive(..)
        | Type::Var(..)
        | Type::DualVar(..)
        | Type::Break(..)
        | Type::Continue(..)
        | Type::Self_(..)
        | Type::DualSelf(..) => {}
    }
    Ok(())
}

pub fn continue_deref<F>(typ: &Type, defs: &TypeDefs, mut visit: F) -> Result<(), TypeError>
where
    F: FnMut(&Type) -> Result<(), TypeError>,
{
    match typ {
        Type::Name(span, name, args) => {
            let typ = defs.get(span, name, args).expect("Type not found");
            visit(&typ)?;
        }
        Type::DualName(span, name, args) => {
            let typ = defs.get_dual(span, name, args).expect("Type not found");
            visit(&typ)?;
        }
        _ => {
            continue_(typ, visit)?;
        }
    }
    Ok(())
}

pub fn continue_deref_polarized<F>(
    typ: &Type,
    is_positive: bool,
    defs: &TypeDefs,
    mut visit: F,
) -> Result<(), TypeError>
where
    F: FnMut(&Type, bool) -> Result<(), TypeError>,
{
    match typ {
        Type::DualName(..) => {
            continue_deref(typ, defs, |child| visit(child, is_positive))?;
        }
        Type::DualBox(_, inner) => {
            visit(inner, !is_positive)?;
        }
        Type::Function(_, left, right) => {
            visit(left, !is_positive)?;
            visit(right, is_positive)?;
        }
        _ => {
            continue_deref(typ, defs, |child| visit(child, is_positive))?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Polarity {
    Positive,
    Negative,
    Both,
    Neither,
}

impl Polarity {
    pub fn dual(self) -> Self {
        match self {
            Polarity::Positive => Polarity::Negative,
            Polarity::Negative => Polarity::Positive,
            Polarity::Both => Polarity::Both,
            Polarity::Neither => Polarity::Neither,
        }
    }
    pub fn xor(self, other: Self) -> Self {
        match self {
            Polarity::Positive => other,
            Polarity::Negative => other.dual(),
            Polarity::Both | Polarity::Neither => self,
        }
    }

    #[allow(dead_code)]
    pub fn is_positive(self) -> bool {
        matches!(self, Polarity::Positive | Polarity::Both)
    }

    #[allow(dead_code)]
    pub fn is_negative(self) -> bool {
        matches!(self, Polarity::Negative | Polarity::Both)
    }
}

fn get_args_polarity(
    span: &Span,
    name: &GlobalName,
    defs: &TypeDefs,
) -> Result<Vec<Polarity>, TypeError> {
    let Some((_span, vars, body)) = defs.globals.get(name) else {
        return Err(TypeError::GlobalNameNotDefined(span.clone(), name.clone()));
    };

    let mut positive_vars: IndexSet<LocalName> = IndexSet::new();
    let mut negative_vars: IndexSet<LocalName> = IndexSet::new();

    fn inner(
        typ: &Type,
        is_positive: bool,
        positive_vars: &mut IndexSet<LocalName>,
        negative_vars: &mut IndexSet<LocalName>,
        names: &IndexSet<LocalName>,
        defs: &TypeDefs,
    ) -> Result<(), TypeError> {
        match typ {
            Type::Var(_, name) if names.contains(name) => {
                if is_positive {
                    positive_vars.insert(name.clone());
                } else {
                    negative_vars.insert(name.clone());
                }
            }
            Type::DualVar(_, name) if names.contains(name) => {
                if is_positive {
                    negative_vars.insert(name.clone());
                } else {
                    positive_vars.insert(name.clone());
                }
            }
            Type::Exists(_, name, body) | Type::Forall(_, name, body) => {
                let mut names = names.clone();
                names.shift_remove(name);
                inner(
                    body,
                    is_positive,
                    positive_vars,
                    negative_vars,
                    &names,
                    defs,
                )?;
            }
            _ => {
                continue_deref_polarized(typ, is_positive, defs, |child, is_positive| {
                    inner(
                        child,
                        is_positive,
                        positive_vars,
                        negative_vars,
                        names,
                        defs,
                    )
                })?;
            }
        }
        Ok(())
    }

    inner(
        body,
        true,
        &mut positive_vars,
        &mut negative_vars,
        &vars.iter().cloned().collect(),
        defs,
    )?;

    Ok(vars
        .iter()
        .map(
            |var| match (positive_vars.contains(var), negative_vars.contains(var)) {
                (true, false) => Polarity::Positive,
                (false, true) => Polarity::Negative,
                (true, true) => Polarity::Both,
                (false, false) => Polarity::Neither,
            },
        )
        .collect())
}

pub fn continue_mut_polarized<F>(
    typ: &mut Type,
    polarity: Polarity,
    defs: &TypeDefs,
    mut visit: F,
) -> Result<(), TypeError>
where
    F: FnMut(&mut Type, Polarity) -> Result<(), TypeError>,
{
    match typ {
        Type::Name(span, name, args) => {
            for (arg, polarity) in args.iter_mut().zip(get_args_polarity(span, name, defs)?) {
                visit(arg, polarity.xor(polarity))?;
            }
        }
        Type::DualName(span, name, args) => {
            for (arg, polarity) in args.iter_mut().zip(get_args_polarity(span, name, defs)?) {
                visit(arg, polarity.xor(polarity).dual())?;
            }
        }
        Type::DualBox(_, inner) => {
            visit(inner, polarity.dual())?;
        }
        Type::Function(_, left, right) => {
            visit(left, polarity.dual())?;
            visit(right, polarity)?;
        }
        _ => {
            continue_mut(typ, |child| visit(child, polarity))?;
        }
    }
    Ok(())
}
