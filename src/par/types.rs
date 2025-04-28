use indexmap::{IndexMap, IndexSet};
use std::{
    collections::HashSet,
    fmt::{self, Write},
    sync::{Arc, RwLock},
};

use super::{language::Name, process};
use crate::location::{Span, Spanning};
use miette::LabeledSpan;

#[derive(Clone, Debug)]
pub enum TypeError {
    TypeNameAlreadyDefined(Span, Span, Name),
    NameAlreadyDeclared(Span, Span, Name),
    NameAlreadyDefined(Span, Span, Name),
    DeclaredButNotDefined(Span, Name),
    NoMatchingRecursiveOrIterative(Span, Option<Name>),
    SelfUsedInNegativePosition(Span, Option<Name>),
    TypeNameNotDefined(Span, Name),
    DependencyCycle(Span, Vec<Name>),
    WrongNumberOfTypeArgs(Span, Name, usize, usize),
    NameNotDefined(Span, Name),
    ShadowedObligation(Span, Name),
    TypeMustBeKnownAtThisPoint(Span, Name),
    ParameterTypeMustBeKnown(Span, Name, Name),
    CannotAssignFromTo(Span, Type, Type),
    UnfulfilledObligations(Span, Vec<Name>),
    InvalidOperation(Span, Operation, Type),
    InvalidBranch(Span, Name, Type),
    MissingBranch(Span, Name, Type),
    RedundantBranch(Span, Name, Type),
    TypesCannotBeUnified(Type, Type),
    NoSuchLoopPoint(Span, Option<Name>),
    DoesNotDescendSubjectOfBegin(Span, Option<Name>),
    LoopVariableNotPreserved(Span, Name),
    LoopVariableChangedType(Span, Name, Type, Type),
    Telltypes(Span, IndexMap<Name, Type>),
}

#[derive(Clone, Debug)]
pub enum Operation {
    Send(Span),
    Receive(Span),
    Choose(Span, Name),
    Match(Span, #[allow(unused)] Arc<[Name]>),
    Break(Span),
    Continue(Span),
    Begin(Span, Option<Name>),
    Loop(Span, Option<Name>),
    SendType(Span),
    ReceiveType(Span),
}

#[derive(Clone, Debug)]
pub enum Type {
    Primitive(Span, PrimitiveType),
    Chan(Span, Box<Self>),
    /// type variable
    Var(Span, Name),
    /// named type
    Name(Span, Name, Vec<Type>),
    Send(Span, Box<Self>, Box<Self>),
    Receive(Span, Box<Self>, Box<Self>),
    Either(Span, IndexMap<Name, Self>),
    Choice(Span, IndexMap<Name, Self>),
    /// ! (unit)
    Break(Span),
    /// ? (bottom)
    Continue(Span),
    Recursive {
        span: Span,
        /*
        The ascendents of the type (denoted by the names of the respective loop points):
        If you `begin` on a `recursive`, and it expands, so its `self`s get replaced by new
        `recursive`s, these new `recursive`s will have as their *ascendent* the original `recursive`.
        This is for totality checking.
         */
        asc: IndexSet<Option<Name>>,
        label: Option<Name>,
        body: Box<Self>,
    },
    Iterative {
        span: Span,
        asc: IndexSet<Option<Name>>,
        label: Option<Name>,
        body: Box<Self>,
    },
    Self_(Span, Option<Name>),
    SendType(Span, Name, Box<Self>),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum PrimitiveType {
    Int,
}

#[derive(Clone, Debug)]
pub struct TypeDefs {
    pub globals: Arc<IndexMap<Name, (Span, Vec<Name>, Type)>>,
    pub vars: IndexSet<Name>,
}

impl Default for TypeDefs {
    fn default() -> Self {
        Self {
            globals: Default::default(),
            vars: Default::default(),
        }
    }
}

impl TypeDefs {
    pub fn new_with_validation<'a>(
        globals: impl Iterator<Item = (&'a Span, &'a Name, &'a Vec<Name>, &'a Type)>,
    ) -> Result<Self, TypeError>
    where
        Name: 'a,
    {
        let mut globals_map = IndexMap::new();
        for (span, name, params, typ) in globals {
            if let Some((span1, _, _)) =
                globals_map.insert(name.clone(), (span.clone(), params.clone(), typ.clone()))
            {
                return Err(TypeError::TypeNameAlreadyDefined(
                    span.clone(),
                    span1,
                    name.clone(),
                ));
            }
        }

        let type_defs = Self {
            globals: Arc::new(globals_map),
            vars: IndexSet::new(),
        };

        for (name, (_, params, typ)) in type_defs.globals.iter() {
            let mut type_defs = type_defs.clone();
            for param in params {
                type_defs.vars.insert(param.clone());
            }
            type_defs.validate_type(
                typ,
                &IndexSet::from([name.clone()]),
                &IndexSet::new(),
                &IndexSet::new(),
            )?;
        }

        Ok(type_defs)
    }

    pub fn get(&self, span: &Span, name: &Name, args: &[Type]) -> Result<Type, TypeError> {
        if self.vars.contains(name) {
            if !args.is_empty() {
                return Err(TypeError::WrongNumberOfTypeArgs(
                    span.clone(),
                    name.clone(),
                    0,
                    args.len(),
                ));
            }
            return Ok(Type::Var(span.clone(), name.clone()));
        }
        match self.globals.get(name) {
            Some((_, params, typ)) => {
                if params.len() != args.len() {
                    return Err(TypeError::WrongNumberOfTypeArgs(
                        span.clone(),
                        name.clone(),
                        params.len(),
                        args.len(),
                    ));
                }
                let mut typ = typ.clone();
                for i in 0..params.len() {
                    typ = typ.substitute(&params[i], &args[i])?;
                }
                Ok(typ)
            }
            None => Err(TypeError::TypeNameNotDefined(span.clone(), name.clone())),
        }
    }

    pub fn get_dual(&self, span: &Span, name: &Name, args: &[Type]) -> Result<Type, TypeError> {
        if self.vars.contains(name) {
            if !args.is_empty() {
                return Err(TypeError::WrongNumberOfTypeArgs(
                    span.clone(),
                    name.clone(),
                    0,
                    args.len(),
                ));
            }
            return Ok(Type::Chan(
                span.clone(),
                Box::new(Type::Var(span.clone(), name.clone())),
            ));
        }
        match self.globals.get(name) {
            Some((_, params, typ)) => {
                if params.len() != args.len() {
                    return Err(TypeError::WrongNumberOfTypeArgs(
                        span.clone(),
                        name.clone(),
                        params.len(),
                        args.len(),
                    ));
                }
                let mut typ = typ.dual(self)?;
                for i in 0..params.len() {
                    typ = typ.substitute(&params[i], &args[i])?;
                }
                Ok(typ)
            }
            None => Err(TypeError::TypeNameNotDefined(span.clone(), name.clone())),
        }
    }

    fn validate_type(
        &self,
        typ: &Type,
        deps: &IndexSet<Name>,
        self_pos: &IndexSet<Option<Name>>,
        self_neg: &IndexSet<Option<Name>>,
    ) -> Result<(), TypeError> {
        Ok(match typ {
            Type::Primitive(_, _) => (),
            Type::Chan(_, t) => self.validate_type(t, deps, self_neg, self_pos)?,
            Type::Var(span, name) => {
                self.get(span, name, &[])?;
            }
            Type::Name(span, name, args) => {
                let mut deps = deps.clone();
                if !self.vars.contains(name) {
                    if !deps.insert(name.clone()) {
                        return Err(TypeError::DependencyCycle(
                            span.clone(),
                            deps.into_iter().skip_while(|dep| dep != name).collect(),
                        ));
                    }
                }
                let t = self.get(span, name, args)?;
                self.validate_type(&t, &deps, self_pos, self_neg)?;
            }
            Type::Send(_, t, u) => {
                self.validate_type(t, deps, self_pos, self_neg)?;
                self.validate_type(u, deps, self_pos, self_neg)?;
            }
            Type::Receive(_, t, u) => {
                self.validate_type(t, deps, self_neg, self_pos)?;
                self.validate_type(u, deps, self_pos, self_neg)?;
            }
            Type::Either(_, branches) | Type::Choice(_, branches) => {
                for (_, t) in branches {
                    self.validate_type(t, deps, self_pos, self_neg)?;
                }
            }
            Type::Break(_) | Type::Continue(_) => (),
            Type::Recursive { label, body, .. } | Type::Iterative { label, body, .. } => {
                let (mut self_pos, mut self_neg) = (self_pos.clone(), self_neg.clone());
                self_pos.insert(label.clone());
                self_neg.shift_remove(label);
                self.validate_type(body, deps, &self_pos, &self_neg)?;
            }
            Type::Self_(span, label) => {
                if self_neg.contains(label) {
                    return Err(TypeError::SelfUsedInNegativePosition(
                        span.clone(),
                        label.clone(),
                    ));
                }
                if !self_pos.contains(label) {
                    return Err(TypeError::NoMatchingRecursiveOrIterative(
                        span.clone(),
                        label.clone(),
                    ));
                }
            }

            Type::SendType(_, name, body) | Type::ReceiveType(_, name, body) => {
                let mut with_var = self.clone();
                with_var.vars.insert(name.clone());
                with_var.validate_type(body, deps, self_pos, self_neg)?;
            }
        })
    }
}

impl Spanning for Type {
    fn span(&self) -> Span {
        match self {
            Self::Primitive(span, _)
            | Self::Chan(span, _)
            | Self::Var(span, _)
            | Self::Name(span, _, _)
            | Self::Send(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Either(span, _)
            | Self::Choice(span, _)
            | Self::Break(span)
            | Self::Continue(span)
            | Self::Recursive { span, .. }
            | Self::Iterative { span, .. }
            | Self::Self_(span, _)
            | Self::SendType(span, _, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

impl Type {
    pub fn substitute(self, var: &Name, typ: &Self) -> Result<Self, TypeError> {
        Ok(match self {
            Self::Primitive(span, p) => Self::Primitive(span, p),
            Self::Chan(span, t) => Self::Chan(span, Box::new(t.substitute(var, typ)?)),
            Self::Var(span, name) => {
                if &name == var {
                    typ.clone()
                } else {
                    Self::Var(span, name)
                }
            }
            Self::Name(span, name, args) if &name == var => {
                if !args.is_empty() {
                    return Err(TypeError::WrongNumberOfTypeArgs(
                        span,
                        var.clone(),
                        0,
                        args.len(),
                    ));
                }
                typ.clone()
            }
            Self::Name(span, name, args) => Self::Name(
                span,
                name,
                args.into_iter()
                    .map(|arg| arg.substitute(var, typ))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Send(loc, t, u) => Self::Send(
                loc,
                Box::new(t.substitute(var, typ)?),
                Box::new(u.substitute(var, typ)?),
            ),
            Self::Receive(loc, t, u) => Self::Receive(
                loc,
                Box::new(t.substitute(var, typ)?),
                Box::new(u.substitute(var, typ)?),
            ),
            Self::Either(span, branches) => Self::Either(
                span,
                branches
                    .into_iter()
                    .map(|(branch, branch_type)| Ok((branch, branch_type.substitute(var, typ)?)))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Choice(span, branches) => Self::Choice(
                span,
                branches
                    .into_iter()
                    .map(|(branch, branch_type)| Ok((branch, branch_type.substitute(var, typ)?)))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Break(span) => Self::Break(span),
            Self::Continue(span) => Self::Continue(span),

            Self::Recursive {
                span,
                asc,
                label,
                body,
            } => Self::Recursive {
                span,
                asc,
                label,
                body: Box::new(body.substitute(var, typ)?),
            },
            Self::Iterative {
                span,
                asc,
                label,
                body,
            } => Self::Iterative {
                span,
                asc,
                label,
                body: Box::new(body.substitute(var, typ)?),
            },
            Self::Self_(span, label) => Self::Self_(span, label),

            Self::SendType(loc, name, body) => {
                if &name == var {
                    Self::SendType(loc, name, body)
                } else {
                    Self::SendType(loc, name, Box::new(body.substitute(var, typ)?))
                }
            }
            Self::ReceiveType(loc, name, body) => {
                if &name == var {
                    Self::ReceiveType(loc, name, body)
                } else {
                    Self::ReceiveType(loc, name, Box::new(body.substitute(var, typ)?))
                }
            }
        })
    }

    pub fn is_linear(&self, type_defs: &TypeDefs) -> Result<bool, TypeError> {
        Ok(!self.is_positive(type_defs)?)
    }

    pub fn is_positive(&self, type_defs: &TypeDefs) -> Result<bool, TypeError> {
        Ok(match self {
            Self::Primitive(_, _) => true,
            Self::Chan(_, t) => t.is_negative(type_defs)?,
            Self::Var(_, _) => false,
            Self::Name(loc, name, args) => {
                type_defs.get(loc, name, args)?.is_positive(type_defs)?
            }
            Self::Send(_, t, u) => t.is_positive(type_defs)? && u.is_positive(type_defs)?,
            Self::Receive(_, _, _) => false,
            Self::Either(_, branches) => {
                for (_, t) in branches {
                    if !t.is_positive(type_defs)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Choice(_, _) => false,
            Self::Break(_) => true,
            Self::Continue(_) => false,
            Self::Recursive { body, .. } => body.is_positive(type_defs)?,
            Self::Iterative { body, .. } => body.is_positive(type_defs)?,
            Self::Self_(_, _) => true,
            Self::SendType(loc, name, t) => t
                .clone()
                .substitute(name, &Type::Var(loc.clone(), name.clone()))?
                .is_positive(type_defs)?,
            Self::ReceiveType(loc, name, t) => t
                .clone()
                .substitute(name, &Type::Var(loc.clone(), name.clone()))?
                .is_positive(type_defs)?,
        })
    }

    pub fn is_negative(&self, type_defs: &TypeDefs) -> Result<bool, TypeError> {
        Ok(match self {
            Self::Primitive(_, _) => true,
            Self::Chan(_, t) => t.is_positive(type_defs)?,
            Self::Var(_, _) => false,
            Self::Name(loc, name, args) => {
                type_defs.get(loc, name, args)?.is_negative(type_defs)?
            }
            Self::Send(_, _, _) => false,
            Self::Receive(_, t, u) => t.is_positive(type_defs)? && u.is_negative(type_defs)?,
            Self::Either(_, _) => false,
            Self::Choice(_, branches) => {
                for (_, t) in branches {
                    if !t.is_negative(type_defs)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Break(_) => false,
            Self::Continue(_) => true,
            Self::Recursive { body, .. } => body.is_negative(type_defs)?,
            Self::Iterative { body, .. } => body.is_negative(type_defs)?,
            Self::Self_(_, _) => true,
            Self::SendType(loc, name, t) => t
                .clone()
                .substitute(name, &Type::Var(loc.clone(), name.clone()))?
                .is_negative(type_defs)?,
            Self::ReceiveType(loc, name, t) => t
                .clone()
                .substitute(name, &Type::Var(loc.clone(), name.clone()))?
                .is_negative(type_defs)?,
        })
    }

    pub fn check_assignable(
        &self,
        span: &Span,
        u: &Type,
        type_defs: &TypeDefs,
    ) -> Result<(), TypeError> {
        if !self.is_assignable_to(u, type_defs, &HashSet::new())? {
            return Err(TypeError::CannotAssignFromTo(
                span.clone(),
                self.clone(),
                u.clone(),
            ));
        }
        Ok(())
    }

    fn is_assignable_to(
        &self,
        other: &Self,
        type_defs: &TypeDefs,
        ind: &HashSet<(Option<Name>, Option<Name>)>,
    ) -> Result<bool, TypeError> {
        Ok(match (self, other) {
            (Self::Chan(_, dual_t1), Self::Chan(_, dual_t2)) => {
                dual_t2.is_assignable_to(dual_t1, type_defs, ind)?
            }
            (Self::Chan(_, dual_t1), t2) => match t2.dual(type_defs)? {
                Self::Chan(_, _) => false,
                dual_t2 => dual_t2.is_assignable_to(dual_t1, type_defs, ind)?,
            },
            (t1, Self::Chan(_, dual_t2)) => match t1.dual(type_defs)? {
                Self::Chan(_, _) => false,
                dual_t1 => dual_t2.is_assignable_to(&dual_t1, type_defs, ind)?,
            },

            (Self::Var(_, name1), Self::Var(_, name2)) => name1 == name2,
            (Self::Name(span, name, args), t2) => type_defs
                .get(span, name, args)?
                .is_assignable_to(t2, type_defs, ind)?,
            (t1, Self::Name(span, name, args)) => {
                t1.is_assignable_to(&type_defs.get(span, name, args)?, type_defs, ind)?
            }

            (Self::Send(_, t1, u1), Self::Send(_, t2, u2)) => {
                t1.is_assignable_to(t2, type_defs, ind)?
                    && u1.is_assignable_to(u2, type_defs, ind)?
            }
            (Self::Receive(_, t1, u1), Self::Receive(_, t2, u2)) => {
                t2.is_assignable_to(t1, type_defs, ind)?
                    && u1.is_assignable_to(u2, type_defs, ind)?
            }
            (Self::Either(_, branches1), Self::Either(_, branches2)) => {
                for (branch, t1) in branches1 {
                    let Some(t2) = branches2.get(branch) else {
                        return Ok(false);
                    };
                    if !t1.is_assignable_to(t2, type_defs, ind)? {
                        return Ok(false);
                    }
                }
                for (branch, _) in branches2 {
                    if branches1.get(branch).is_none() {
                        return Ok(false);
                    }
                }
                true
            }
            (Self::Choice(_, branches1), Self::Choice(_, branches2)) => {
                for (branch, _) in branches1 {
                    if branches2.get(branch).is_none() {
                        return Ok(false);
                    }
                }
                for (branch, t2) in branches2 {
                    let Some(t1) = branches1.get(branch) else {
                        return Ok(false);
                    };
                    if !t1.is_assignable_to(t2, type_defs, ind)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Self::Break(_), Self::Break(_)) => true,
            (Self::Continue(_), Self::Continue(_)) => true,

            (
                Self::Recursive {
                    asc: asc1,
                    label: label1,
                    body: body1,
                    ..
                },
                Self::Recursive {
                    asc: asc2,
                    label: label2,
                    body: body2,
                    ..
                },
            ) => {
                if !asc2.iter().all(|label| asc1.contains(label)) {
                    return Ok(false);
                }
                let mut ind = ind.clone();
                ind.insert((label1.clone(), label2.clone()));
                body1.is_assignable_to(body2, type_defs, &ind)?
            }
            (
                typ,
                Self::Recursive {
                    asc, label, body, ..
                },
            ) => typ.is_assignable_to(
                &Self::expand_recursive(asc, label, body, type_defs)?,
                type_defs,
                ind,
            )?,
            (
                Self::Iterative {
                    asc: asc1,
                    label: label1,
                    body: body1,
                    ..
                },
                Self::Iterative {
                    asc: asc2,
                    label: label2,
                    body: body2,
                    ..
                },
            ) => {
                if !asc2.iter().all(|label| asc1.contains(label)) {
                    return Ok(false);
                }
                let mut ind = ind.clone();
                ind.insert((label1.clone(), label2.clone()));
                body1.is_assignable_to(body2, type_defs, &ind)?
            }
            (
                Self::Iterative {
                    asc, label, body, ..
                },
                typ,
            ) => Self::expand_iterative(asc, label, body, type_defs)?
                .is_assignable_to(typ, type_defs, ind)?,

            (Self::Self_(_, label1), Self::Self_(_, label2)) => {
                ind.contains(&(label1.clone(), label2.clone()))
            }

            (Self::SendType(loc, name1, body1), Self::SendType(_, name2, body2))
            | (Self::ReceiveType(loc, name1, body1), Self::ReceiveType(_, name2, body2)) => {
                let body2 = body2
                    .clone()
                    .substitute(name2, &Type::Var(loc.clone(), name1.clone()))?;
                let mut type_defs = type_defs.clone();
                type_defs.vars.insert(name1.clone());
                body1.is_assignable_to(&body2, &type_defs, ind)?
            }

            _ => false,
        })
    }

    pub fn dual(&self, type_defs: &TypeDefs) -> Result<Self, TypeError> {
        Ok(match self {
            Self::Primitive(span, p) => Self::Chan(
                span.clone(),
                Box::new(Self::Primitive(span.clone(), p.clone())),
            ),

            Self::Chan(_, t) => *t.clone(),

            Self::Var(span, name) => Self::Chan(
                span.clone(),
                Box::new(Self::Var(span.clone(), name.clone())),
            ),
            Self::Name(span, name, args) => match type_defs.get_dual(span, name, args) {
                Ok(dual) => dual,
                Err(_) => Self::Chan(
                    span.clone(),
                    Box::new(Self::Name(span.clone(), name.clone(), args.clone())),
                ),
            },

            Self::Send(loc, t, u) => {
                Self::Receive(loc.clone(), t.clone(), Box::new(u.dual(type_defs)?))
            }
            Self::Receive(loc, t, u) => {
                Self::Send(loc.clone(), t.clone(), Box::new(u.dual(type_defs)?))
            }
            Self::Either(span, branches) => Self::Choice(
                span.clone(),
                branches
                    .iter()
                    .map(|(branch, t)| Ok((branch.clone(), t.dual(type_defs)?)))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Choice(span, branches) => Self::Either(
                span.clone(),
                branches
                    .iter()
                    .map(|(branch, t)| Ok((branch.clone(), t.dual(type_defs)?)))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Break(span) => Self::Continue(span.clone()),
            Self::Continue(span) => Self::Break(span.clone()),

            Self::Recursive {
                span,
                asc,
                label,
                body: t,
            } => Self::Iterative {
                span: span.clone(),
                asc: asc.clone(),
                label: label.clone(),
                body: Box::new(t.dual(type_defs)?.chan_self(label)),
            },
            Self::Iterative {
                span,
                asc,
                label,
                body: t,
            } => Self::Recursive {
                span: span.clone(),
                asc: asc.clone(),
                label: label.clone(),
                body: Box::new(t.dual(type_defs)?.chan_self(label)),
            },
            Self::Self_(span, label) => Self::Chan(
                span.clone(),
                Box::new(Self::Self_(span.clone(), label.clone())),
            ),

            Self::SendType(loc, name, t) => {
                Self::ReceiveType(loc.clone(), name.clone(), Box::new(t.dual(type_defs)?))
            }
            Self::ReceiveType(loc, name, t) => {
                Self::SendType(loc.clone(), name.clone(), Box::new(t.dual(type_defs)?))
            }
        })
    }

    fn chan_self(self, label: &Option<Name>) -> Self {
        match self {
            Self::Primitive(span, p) => Self::Primitive(span, p),

            Self::Chan(span, t) => match *t {
                Self::Self_(span, label1) if &label1 == label => Self::Self_(span, label1),
                t => Self::Chan(span, Box::new(t.chan_self(label))),
            },

            Self::Var(span, name) => Self::Var(span, name),
            Self::Name(span, name, args) => Self::Name(
                span.clone(),
                name.clone(),
                args.into_iter().map(|arg| arg.chan_self(label)).collect(),
            ),

            Self::Send(loc, t, u) => Self::Send(
                loc.clone(),
                Box::new(t.chan_self(label)),
                Box::new(u.chan_self(label)),
            ),
            Self::Receive(loc, t, u) => Self::Receive(
                loc.clone(),
                Box::new(t.chan_self(label)),
                Box::new(u.chan_self(label)),
            ),
            Self::Either(span, branches) => Self::Either(
                span.clone(),
                branches
                    .into_iter()
                    .map(|(branch, t)| (branch, t.chan_self(label)))
                    .collect(),
            ),
            Self::Choice(span, branches) => Self::Choice(
                span.clone(),
                branches
                    .into_iter()
                    .map(|(branch, t)| (branch, t.chan_self(label)))
                    .collect(),
            ),
            Self::Break(span) => Self::Break(span.clone()),
            Self::Continue(span) => Self::Continue(span.clone()),

            Self::Recursive {
                span,
                asc,
                label: label1,
                body: t,
            } => {
                if &label1 == label {
                    Self::Recursive {
                        span,
                        asc,
                        label: label1,
                        body: t,
                    }
                } else {
                    Self::Recursive {
                        span,
                        asc,
                        label: label1,
                        body: Box::new(t.chan_self(label)),
                    }
                }
            }
            Self::Iterative {
                span,
                asc,
                label: label1,
                body: t,
            } => {
                if &label1 == label {
                    Self::Iterative {
                        span,
                        asc,
                        label: label1,
                        body: t,
                    }
                } else {
                    Self::Iterative {
                        span,
                        asc,
                        label: label1,
                        body: Box::new(t.chan_self(label)),
                    }
                }
            }
            Self::Self_(span, label1) => {
                if &label1 == label {
                    Self::Chan(span.clone(), Box::new(Self::Self_(span, label1)))
                } else {
                    Self::Self_(span, label1)
                }
            }

            Self::SendType(loc, name, t) => {
                Self::SendType(loc.clone(), name.clone(), Box::new(t.chan_self(label)))
            }
            Self::ReceiveType(loc, name, t) => {
                Self::ReceiveType(loc.clone(), name.clone(), Box::new(t.chan_self(label)))
            }
        }
    }

    pub fn expand_recursive(
        asc: &IndexSet<Option<Name>>,
        label: &Option<Name>,
        body: &Self,
        type_defs: &TypeDefs,
    ) -> Result<Self, TypeError> {
        body.clone()
            .expand_recursive_helper(asc, label, body, type_defs)
    }

    fn expand_recursive_helper(
        self,
        top_asc: &IndexSet<Option<Name>>,
        top_label: &Option<Name>,
        top_body: &Self,
        type_defs: &TypeDefs,
    ) -> Result<Self, TypeError> {
        Ok(match self {
            Self::Primitive(span, p) => Self::Primitive(span, p),

            Self::Chan(span, t) => match *t {
                Self::Self_(span, label) if &label == top_label => Self::Iterative {
                    span,
                    asc: top_asc.clone(),
                    label: label.clone(),
                    body: Box::new(top_body.dual(type_defs)?.chan_self(&label)),
                },
                t => Self::Chan(
                    span,
                    Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
                ),
            },

            Self::Var(span, name) => Self::Var(span, name),
            Self::Name(span, name, args) => Self::Name(
                span,
                name,
                args.into_iter()
                    .map(|arg| {
                        Ok(arg.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?)
                    })
                    .collect::<Result<_, _>>()?,
            ),

            Self::Send(loc, t, u) => Self::Send(
                loc,
                Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
                Box::new(u.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::Receive(loc, t, u) => Self::Receive(
                loc,
                Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
                Box::new(u.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::Either(span, branches) => Self::Either(
                span,
                branches
                    .into_iter()
                    .map(|(branch, typ)| {
                        Ok((
                            branch,
                            typ.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?,
                        ))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Choice(span, branches) => Self::Choice(
                span,
                branches
                    .into_iter()
                    .map(|(branch, typ)| {
                        Ok((
                            branch,
                            typ.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?,
                        ))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Break(span) => Self::Break(span),
            Self::Continue(span) => Self::Continue(span),

            Self::Recursive {
                span,
                asc,
                label,
                body: t,
            } => {
                if &label == top_label {
                    Self::Recursive {
                        span,
                        asc,
                        label,
                        body: t,
                    }
                } else {
                    Self::Recursive {
                        span,
                        asc,
                        label,
                        body: Box::new(
                            t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?,
                        ),
                    }
                }
            }
            Self::Iterative {
                span,
                asc,
                label,
                body: t,
            } => Self::Iterative {
                span,
                asc,
                label,
                body: Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
            },
            Self::Self_(span, label) => {
                if &label == top_label {
                    Self::Recursive {
                        span,
                        asc: top_asc.clone(),
                        label,
                        body: Box::new(top_body.clone()),
                    }
                } else {
                    Self::Self_(span, label)
                }
            }

            Self::SendType(loc, name, t) => Self::SendType(
                loc,
                name,
                Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::ReceiveType(loc, name, t) => Self::ReceiveType(
                loc,
                name,
                Box::new(t.expand_recursive_helper(top_asc, top_label, top_body, type_defs)?),
            ),
        })
    }

    pub fn expand_iterative(
        asc: &IndexSet<Option<Name>>,
        label: &Option<Name>,
        body: &Self,
        type_defs: &TypeDefs,
    ) -> Result<Self, TypeError> {
        body.clone()
            .expand_iterative_helper(asc, label, body, type_defs)
    }

    fn expand_iterative_helper(
        self,
        top_asc: &IndexSet<Option<Name>>,
        top_label: &Option<Name>,
        top_body: &Self,
        type_defs: &TypeDefs,
    ) -> Result<Self, TypeError> {
        Ok(match self {
            Self::Primitive(span, p) => Self::Primitive(span, p),

            Self::Chan(span, t) => match *t {
                Self::Self_(span, label) if &label == top_label => Self::Recursive {
                    span,
                    asc: top_asc.clone(),
                    label: label.clone(),
                    body: Box::new(top_body.dual(type_defs)?.chan_self(&label)),
                },
                t => Self::Chan(
                    span,
                    Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
                ),
            },

            Self::Var(span, name) => Self::Var(span, name),
            Self::Name(span, name, args) => Self::Name(
                span,
                name,
                args.into_iter()
                    .map(|arg| {
                        Ok(arg.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?)
                    })
                    .collect::<Result<_, _>>()?,
            ),

            Self::Send(loc, t, u) => Self::Send(
                loc,
                Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
                Box::new(u.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::Receive(loc, t, u) => Self::Receive(
                loc,
                Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
                Box::new(u.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::Either(span, branches) => Self::Either(
                span,
                branches
                    .into_iter()
                    .map(|(branch, typ)| {
                        Ok((
                            branch,
                            typ.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?,
                        ))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Choice(span, branches) => Self::Choice(
                span,
                branches
                    .into_iter()
                    .map(|(branch, typ)| {
                        Ok((
                            branch,
                            typ.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?,
                        ))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Break(span) => Self::Break(span),
            Self::Continue(span) => Self::Continue(span),

            Self::Recursive {
                span,
                asc,
                label,
                body: t,
            } => Self::Recursive {
                span,
                asc,
                label,
                body: Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
            },
            Self::Iterative {
                span,
                asc,
                label,
                body: t,
            } => {
                if &label == top_label {
                    Self::Iterative {
                        span,
                        asc,
                        label,
                        body: t,
                    }
                } else {
                    Self::Iterative {
                        span,
                        asc,
                        label,
                        body: Box::new(
                            t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?,
                        ),
                    }
                }
            }
            Self::Self_(span, label) => {
                if &label == top_label {
                    Self::Iterative {
                        span,
                        asc: top_asc.clone(),
                        label,
                        body: Box::new(top_body.clone()),
                    }
                } else {
                    Self::Self_(span, label)
                }
            }

            Self::SendType(loc, name, t) => Self::SendType(
                loc,
                name,
                Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
            ),
            Self::ReceiveType(loc, name, t) => Self::ReceiveType(
                loc,
                name,
                Box::new(t.expand_iterative_helper(top_asc, top_label, top_body, type_defs)?),
            ),
        })
    }

    fn invalidate_ascendent(&mut self, label: &Option<Name>) {
        match self {
            Self::Primitive(_, _) => {}
            Self::Var(_, _) => {}
            Self::Name(_, _, args) => {
                for arg in args {
                    arg.invalidate_ascendent(label);
                }
            }
            Self::Send(_, t, u) => {
                t.invalidate_ascendent(label);
                u.invalidate_ascendent(label);
            }
            Self::Receive(_, t, u) => {
                t.invalidate_ascendent(label);
                u.invalidate_ascendent(label);
            }
            Self::Either(_, branches) => {
                for (_, t) in branches {
                    t.invalidate_ascendent(label);
                }
            }
            Self::Choice(_, branches) => {
                for (_, t) in branches {
                    t.invalidate_ascendent(label);
                }
            }
            Self::Break(_) => {}
            Self::Continue(_) => {}

            Self::Recursive {
                span: _,
                asc,
                label: _,
                body: t,
            } => {
                asc.shift_remove(label);
                t.invalidate_ascendent(label);
            }
            Self::Iterative {
                span: _,
                asc,
                label: _,
                body: t,
            } => {
                asc.shift_remove(label);
                t.invalidate_ascendent(label);
            }
            Self::Self_(_, _) => {}

            Self::SendType(_, _, t) => {
                t.invalidate_ascendent(label);
            }
            Self::ReceiveType(_, _, t) => {
                t.invalidate_ascendent(label);
            }

            Self::Chan(_, t) => {
                t.invalidate_ascendent(label);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    type_defs: TypeDefs,
    declarations: Arc<IndexMap<Name, (Span, Type)>>,
    unchecked_definitions: Arc<IndexMap<Name, (Span, Arc<process::Expression<()>>)>>,
    checked_definitions: Arc<RwLock<IndexMap<Name, CheckedDef>>>,
    current_deps: IndexSet<Name>,
    variables: IndexMap<Name, Type>,
    loop_points: IndexMap<Option<Name>, (Name, Arc<IndexMap<Name, Type>>)>,
}

#[derive(Clone, Debug)]
struct CheckedDef {
    span: Span,
    def: Arc<process::Expression<Type>>,
    typ: Type,
}

impl Context {
    pub fn new(
        type_defs: TypeDefs,
        declarations: IndexMap<Name, (Span, Type)>,
        unchecked_definitions: IndexMap<Name, (Span, Arc<process::Expression<()>>)>,
    ) -> Self {
        Self {
            type_defs,
            declarations: Arc::new(declarations),
            unchecked_definitions: Arc::new(unchecked_definitions),
            checked_definitions: Arc::new(RwLock::new(IndexMap::new())),
            current_deps: IndexSet::new(),
            variables: IndexMap::new(),
            loop_points: IndexMap::new(),
        }
    }

    pub fn check_definition(&mut self, span: &Span, name: &Name) -> Result<Type, TypeError> {
        if let Some(checked) = self.checked_definitions.read().unwrap().get(name) {
            return Ok(checked.typ.clone());
        }

        let Some((span_def, unchecked_def)) = self.unchecked_definitions.get(name).cloned() else {
            return Err(TypeError::NameNotDefined(span.clone(), name.clone()));
        };

        if !self.current_deps.insert(name.clone()) {
            return Err(TypeError::DependencyCycle(
                span.clone(),
                self.current_deps
                    .iter()
                    .cloned()
                    .skip_while(|dep| dep != name)
                    .collect(),
            ));
        }

        let (checked_def, checked_type) = match self.declarations.get(name).cloned() {
            Some((_, declared_type)) => {
                let checked_def = self.check_expression(None, &unchecked_def, &declared_type)?;
                (checked_def, declared_type)
            }
            None => self.infer_expression(None, &unchecked_def)?,
        };

        self.checked_definitions.write().unwrap().insert(
            name.clone(),
            CheckedDef {
                span: span_def,
                def: checked_def,
                typ: checked_type.clone(),
            },
        );

        Ok(checked_type)
    }

    pub fn get_checked_definitions(
        &self,
    ) -> IndexMap<Name, (Span, Arc<process::Expression<Type>>)> {
        self.checked_definitions
            .read()
            .unwrap()
            .iter()
            .map(|(name, checked)| (name.clone(), (checked.span.clone(), checked.def.clone())))
            .collect()
    }

    pub fn get_declarations(&self) -> IndexMap<Name, (Span, Type)> {
        (*self.declarations).clone()
    }

    pub fn get_type_defs(&self) -> &TypeDefs {
        &self.type_defs
    }

    pub fn split(&self) -> Self {
        Self {
            type_defs: self.type_defs.clone(),
            declarations: self.declarations.clone(),
            unchecked_definitions: self.unchecked_definitions.clone(),
            checked_definitions: self.checked_definitions.clone(),
            current_deps: self.current_deps.clone(),
            variables: IndexMap::new(),
            loop_points: self.loop_points.clone(),
        }
    }

    pub fn get_variable(&mut self, name: &Name) -> Option<Type> {
        self.variables.shift_remove(name)
    }

    pub fn get(&mut self, span: &Span, name: &Name) -> Result<Type, TypeError> {
        match self.get_variable(name) {
            Some(typ) => Ok(typ),
            None => self.check_definition(span, name),
        }
    }

    pub fn put(&mut self, span: &Span, name: Name, typ: Type) -> Result<(), TypeError> {
        if let Some(typ) = self.variables.get(&name) {
            if typ.is_linear(&self.type_defs)? {
                return Err(TypeError::ShadowedObligation(span.clone(), name));
            }
        }
        self.variables.insert(name, typ);
        Ok(())
    }

    fn invalidate_ascendent(&mut self, label: &Option<Name>) {
        for (_, t) in &mut self.variables {
            t.invalidate_ascendent(label);
        }
    }

    pub fn capture(
        &mut self,
        inference_subject: Option<&Name>,
        cap: &process::Captures,
        target: &mut Self,
    ) -> Result<(), TypeError> {
        for (name, span) in &cap.names {
            if Some(name) == inference_subject {
                return Err(TypeError::TypeMustBeKnownAtThisPoint(
                    span.clone(),
                    name.clone(),
                ));
            }
            let typ = match self.get_variable(name) {
                Some(typ) => typ,
                None => continue,
            };
            if !typ.is_linear(&self.type_defs)? {
                self.put(span, name.clone(), typ.clone())?;
            }
            target.put(span, name.clone(), typ)?;
        }
        Ok(())
    }

    pub fn obligations(&self) -> impl Iterator<Item = &Name> {
        self.variables
            .iter()
            .filter(|(_, typ)| typ.is_linear(&self.type_defs).ok().unwrap_or(true))
            .map(|(name, _)| name)
    }

    pub fn check_process(
        &mut self,
        process: &process::Process<()>,
    ) -> Result<Arc<process::Process<Type>>, TypeError> {
        match process {
            process::Process::Let {
                span,
                name,
                annotation,
                typ: (),
                value: expression,
                then: process,
            } => {
                let (expression, typ) = match annotation {
                    Some(annotated_type) => (
                        self.check_expression(None, expression, annotated_type)?,
                        annotated_type.clone(),
                    ),
                    None => self.infer_expression(None, expression)?,
                };
                self.put(span, name.clone(), typ.clone())?;
                let process = self.check_process(process)?;
                Ok(Arc::new(process::Process::Let {
                    span: span.clone(),
                    name: name.clone(),
                    annotation: annotation.clone(),
                    typ: typ,
                    value: expression,
                    then: process,
                }))
            }

            process::Process::Do {
                span,
                name: object,
                typ: (),
                command,
            } => {
                let typ = self.get(span, object)?;

                let (command, _) = self.check_command(
                    None,
                    span,
                    object,
                    &typ,
                    command,
                    &mut |context, process| Ok((context.check_process(process)?, None)),
                )?;

                Ok(Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object.clone(),
                    typ: typ,
                    command: command,
                }))
            }

            process::Process::Telltypes(span, _) => {
                Err(TypeError::Telltypes(span.clone(), self.variables.clone()))
            }
        }
    }

    fn check_command(
        &mut self,
        inference_subject: Option<&Name>,
        span: &Span,
        object: &Name,
        typ: &Type,
        command: &process::Command<()>,
        analyze_process: &mut impl FnMut(
            &mut Self,
            &process::Process<()>,
        ) -> Result<
            (Arc<process::Process<Type>>, Option<Type>),
            TypeError,
        >,
    ) -> Result<(process::Command<Type>, Option<Type>), TypeError> {
        if let Type::Name(_, name, args) = typ {
            return self.check_command(
                inference_subject,
                span,
                object,
                &self.type_defs.get(span, name, args)?,
                command,
                analyze_process,
            );
        }
        if !matches!(command, process::Command::Link(_)) {
            if let Type::Iterative {
                asc: top_asc,
                label: top_label,
                body,
                ..
            } = typ
            {
                return self.check_command(
                    inference_subject,
                    span,
                    object,
                    &Type::expand_iterative(top_asc, top_label, body, &self.type_defs)?,
                    command,
                    analyze_process,
                );
            }
        }
        if !matches!(
            command,
            process::Command::Begin { .. } | process::Command::Loop(_, _)
        ) {
            if let Type::Recursive {
                asc: top_asc,
                label: top_label,
                body,
                ..
            } = typ
            {
                return self.check_command(
                    inference_subject,
                    span,
                    object,
                    &Type::expand_recursive(top_asc, top_label, body, &self.type_defs)?,
                    command,
                    analyze_process,
                );
            }
        }
        if let Type::Chan(_, dual_typ) = typ {
            match dual_typ.dual(&self.type_defs)? {
                Type::Chan(_, _) => {}
                typ => {
                    return self.check_command(
                        inference_subject,
                        span,
                        object,
                        &typ,
                        command,
                        analyze_process,
                    )
                }
            }
        }

        Ok(match command {
            process::Command::Link(expression) => {
                let expression =
                    self.check_expression(None, expression, &typ.dual(&self.type_defs)?)?;
                self.cannot_have_obligations(span)?;
                (process::Command::Link(expression), None)
            }

            process::Command::Send(argument, process) => {
                let Type::Receive(_, argument_type, then_type) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Send(span.clone()),
                        typ.clone(),
                    ));
                };
                let argument = self.check_expression(None, argument, &argument_type)?;
                self.put(span, object.clone(), *then_type.clone())?;
                let (process, inferred_types) = analyze_process(self, process)?;
                (process::Command::Send(argument, process), inferred_types)
            }

            process::Command::Receive(parameter, annotation, (), process) => {
                let Type::Send(_, parameter_type, then_type) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Receive(span.clone()),
                        typ.clone(),
                    ));
                };
                if let Some(annotated_type) = annotation {
                    parameter_type.check_assignable(span, annotated_type, &self.type_defs)?;
                }
                self.put(span, parameter.clone(), *parameter_type.clone())?;
                self.put(span, object.clone(), *then_type.clone())?;
                let (process, inferred_types) = analyze_process(self, process)?;
                (
                    process::Command::Receive(
                        parameter.clone(),
                        annotation.clone(),
                        *parameter_type.clone(),
                        process,
                    ),
                    inferred_types,
                )
            }

            process::Command::Choose(chosen, process) => {
                let Type::Choice(_, branches) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Choose(span.clone(), chosen.clone()),
                        typ.clone(),
                    ));
                };
                let Some(branch_type) = branches.get(chosen) else {
                    return Err(TypeError::InvalidBranch(
                        span.clone(),
                        chosen.clone(),
                        typ.clone(),
                    ));
                };
                self.put(span, object.clone(), branch_type.clone())?;
                let (process, inferred_types) = analyze_process(self, process)?;
                (
                    process::Command::Choose(chosen.clone(), process),
                    inferred_types,
                )
            }

            process::Command::Match(branches, processes) => {
                let Type::Either(_, required_branches) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Match(span.clone(), Arc::clone(branches)),
                        typ.clone(),
                    ));
                };
                if let Some(missing) = required_branches
                    .keys()
                    .find(|&branch| !branches.contains(branch))
                {
                    return Err(TypeError::MissingBranch(
                        span.clone(),
                        missing.clone(),
                        typ.clone(),
                    ));
                }

                let original_context = self.clone();
                let mut typed_processes = Vec::new();
                let mut inferred_type: Option<Type> = None;

                for (branch, process) in branches.iter().zip(processes.iter()) {
                    *self = original_context.clone();

                    let Some(branch_type) = required_branches.get(branch) else {
                        return Err(TypeError::RedundantBranch(
                            span.clone(),
                            branch.clone(),
                            typ.clone(),
                        ));
                    };
                    self.put(span, object.clone(), branch_type.clone())?;
                    let (process, inferred_in_branch) = analyze_process(self, process)?;
                    typed_processes.push(process);

                    match (inferred_type, inferred_in_branch) {
                        (None, Some(t2)) => inferred_type = Some(t2),
                        (Some(t1), Some(t2))
                            if t1.is_assignable_to(&t2, &self.type_defs, &HashSet::new())? =>
                        {
                            inferred_type = Some(t2)
                        }
                        (Some(t1), Some(t2))
                            if !t2.is_assignable_to(&t1, &self.type_defs, &HashSet::new())? =>
                        {
                            return Err(TypeError::TypesCannotBeUnified(t1, t2))
                        }
                        (t1, _) => inferred_type = t1,
                    }
                }

                (
                    process::Command::Match(Arc::clone(branches), Box::from(typed_processes)),
                    inferred_type,
                )
            }

            process::Command::Break => {
                let Type::Continue(_) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Break(span.clone()),
                        typ.clone(),
                    ));
                };
                self.cannot_have_obligations(span)?;
                (process::Command::Break, None)
            }

            process::Command::Continue(process) => {
                let Type::Break(_) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Continue(span.clone()),
                        typ.clone(),
                    ));
                };
                let (process, inferred_types) = analyze_process(self, process)?;
                (process::Command::Continue(process), inferred_types)
            }

            process::Command::Begin {
                unfounded,
                label,
                captures,
                body: process,
            } => {
                let Type::Recursive {
                    span: typ_span,
                    asc: typ_asc,
                    label: typ_label,
                    body: typ_body,
                } = typ
                else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Begin(span.clone(), label.clone()),
                        typ.clone(),
                    ));
                };

                let mut typ_asc = typ_asc.clone();

                if !*unfounded {
                    typ_asc.insert(label.clone());
                }

                self.invalidate_ascendent(label);
                self.loop_points.insert(
                    label.clone(),
                    (
                        object.clone(),
                        Arc::new({
                            let mut variables = self
                                .variables
                                .iter()
                                .filter(|&(name, _)| captures.names.contains_key(name))
                                .map(|(name, typ)| (name.clone(), typ.clone()))
                                .collect::<IndexMap<_, _>>();
                            variables.insert(
                                object.clone(),
                                Type::Recursive {
                                    span: typ_span.clone(),
                                    asc: typ_asc.clone(),
                                    label: typ_label.clone(),
                                    body: typ_body.clone(),
                                },
                            );
                            variables
                        }),
                    ),
                );

                self.put(
                    span,
                    object.clone(),
                    Type::expand_recursive(&typ_asc, typ_label, typ_body, &self.type_defs)?,
                )?;
                let (process, inferred_type) = analyze_process(self, process)?;

                let inferred_iterative = inferred_type.map(|body| Type::Iterative {
                    span: span.clone(),
                    asc: typ_asc,
                    label: label.clone(),
                    body: Box::new(body),
                });

                (
                    process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone(),
                        captures: captures.clone(),
                        body: process,
                    },
                    inferred_iterative,
                )
            }

            process::Command::Loop(label, captures) => {
                if !matches!(typ, Type::Recursive { .. }) {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::Loop(span.clone(), label.clone()),
                        typ.clone(),
                    ));
                }
                let Some((driver, variables)) = self.loop_points.get(label).cloned() else {
                    return Err(TypeError::NoSuchLoopPoint(span.clone(), label.clone()));
                };
                self.put(span, driver.clone(), typ.clone())?;

                if let (
                    Type::Recursive { asc: asc1, .. },
                    Some(Type::Recursive { asc: asc2, .. }),
                ) = (typ, variables.get(&driver))
                {
                    for label in asc2 {
                        if !asc1.contains(label) {
                            return Err(TypeError::DoesNotDescendSubjectOfBegin(
                                span.clone(),
                                label.clone(),
                            ));
                        }
                    }
                }

                let mut inferred_loop = None;

                for (var, type_at_begin) in variables.as_ref() {
                    if Some(var) == inference_subject {
                        inferred_loop = Some(type_at_begin.clone());
                        continue;
                    }
                    let Some(current_type) = self.get_variable(var) else {
                        return Err(TypeError::LoopVariableNotPreserved(
                            span.clone(),
                            var.clone(),
                        ));
                    };
                    if !current_type.is_assignable_to(
                        type_at_begin,
                        &self.type_defs,
                        &HashSet::new(),
                    )? {
                        return Err(TypeError::LoopVariableChangedType(
                            span.clone(),
                            var.clone(),
                            current_type,
                            type_at_begin.clone(),
                        ));
                    }
                }
                self.cannot_have_obligations(span)?;

                (
                    process::Command::Loop(label.clone(), captures.clone()),
                    inferred_loop.or(Some(Type::Self_(span.clone(), label.clone()))),
                )
            }

            process::Command::SendType(argument, process) => {
                let Type::ReceiveType(_, type_name, then_type) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::SendType(span.clone()),
                        typ.clone(),
                    ));
                };
                let then_type = then_type.clone().substitute(type_name, argument)?;
                self.put(span, object.clone(), then_type)?;
                let (process, inferred_types) = analyze_process(self, process)?;
                (
                    process::Command::SendType(argument.clone(), process),
                    inferred_types,
                )
            }

            process::Command::ReceiveType(parameter, process) => {
                let Type::SendType(_, type_name, then_type) = typ else {
                    return Err(TypeError::InvalidOperation(
                        span.clone(),
                        Operation::ReceiveType(span.clone()),
                        typ.clone(),
                    ));
                };
                let then_type = then_type
                    .clone()
                    .substitute(type_name, &Type::Var(span.clone(), parameter.clone()))?;
                self.type_defs.vars.insert(parameter.clone());
                self.put(span, object.clone(), then_type)?;
                let (process, inferred_types) = analyze_process(self, process)?;
                (
                    process::Command::ReceiveType(parameter.clone(), process),
                    inferred_types,
                )
            }
        })
    }

    pub fn infer_process(
        &mut self,
        process: &process::Process<()>,
        subject: &Name,
    ) -> Result<(Arc<process::Process<Type>>, Type), TypeError> {
        match process {
            process::Process::Let {
                span,
                name,
                annotation,
                typ: (),
                value: expression,
                then: process,
            } => {
                let (expression, typ) = match annotation {
                    Some(annotated_type) => (
                        self.check_expression(Some(subject), expression, annotated_type)?,
                        annotated_type.clone(),
                    ),
                    None => self.infer_expression(Some(subject), expression)?,
                };
                self.put(span, name.clone(), typ.clone())?;
                let (process, subject_type) = self.infer_process(process, subject)?;
                Ok((
                    Arc::new(process::Process::Let {
                        span: span.clone(),
                        name: name.clone(),
                        annotation: annotation.clone(),
                        typ,
                        value: expression,
                        then: process,
                    }),
                    subject_type,
                ))
            }

            process::Process::Do {
                span,
                name: object,
                typ: (),
                command,
            } => {
                if object == subject {
                    let (command, typ) = self.infer_command(span, subject, command)?;
                    return Ok((
                        Arc::new(process::Process::Do {
                            span: span.clone(),
                            name: object.clone(),
                            typ: typ.clone(),
                            command,
                        }),
                        typ,
                    ));
                }
                let typ = self.get(span, object)?;

                let (command, inferred_type) = self.check_command(
                    Some(subject),
                    span,
                    object,
                    &typ,
                    command,
                    &mut |context, process| {
                        let (process, typ) = context.infer_process(process, subject)?;
                        Ok((process, Some(typ)))
                    },
                )?;

                let Some(inferred_type) = inferred_type else {
                    return Err(TypeError::TypeMustBeKnownAtThisPoint(
                        span.clone(),
                        subject.clone(),
                    ));
                };

                Ok((
                    Arc::new(process::Process::Do {
                        span: span.clone(),
                        name: object.clone(),
                        typ,
                        command,
                    }),
                    inferred_type,
                ))
            }

            process::Process::Telltypes(span, _) => {
                Err(TypeError::Telltypes(span.clone(), self.variables.clone()))
            }
        }
    }

    pub fn infer_command(
        &mut self,
        span: &Span,
        subject: &Name,
        command: &process::Command<()>,
    ) -> Result<(process::Command<Type>, Type), TypeError> {
        Ok(match command {
            process::Command::Link(expression) => {
                let (expression, typ) = self.infer_expression(Some(subject), expression)?;
                (
                    process::Command::Link(expression),
                    typ.dual(&self.type_defs)?,
                )
            }

            process::Command::Send(argument, process) => {
                let (argument, arg_type) = self.infer_expression(Some(subject), argument)?;
                let (process, then_type) = self.infer_process(process, subject)?;
                (
                    process::Command::Send(argument, process),
                    Type::Receive(span.clone(), Box::new(arg_type), Box::new(then_type)),
                )
            }

            process::Command::Receive(parameter, annotation, (), process) => {
                let Some(param_type) = annotation else {
                    return Err(TypeError::ParameterTypeMustBeKnown(
                        span.clone(),
                        subject.clone(),
                        parameter.clone(),
                    ));
                };
                self.put(span, parameter.clone(), param_type.clone())?;
                let (process, then_type) = self.infer_process(process, subject)?;
                (
                    process::Command::Receive(
                        parameter.clone(),
                        annotation.clone(),
                        param_type.clone(),
                        process,
                    ),
                    Type::Send(
                        span.clone(),
                        Box::new(param_type.clone()),
                        Box::new(then_type),
                    ),
                )
            }

            process::Command::Choose(_, _) => {
                return Err(TypeError::TypeMustBeKnownAtThisPoint(
                    span.clone(),
                    subject.clone(),
                ))
            }

            process::Command::Match(branches, processes) => {
                let original_context = self.clone();
                let mut typed_processes = Vec::new();
                let mut branch_types = IndexMap::new();

                for (branch, process) in branches.iter().zip(processes.iter()) {
                    *self = original_context.clone();
                    let (process, typ) = self.infer_process(process, subject)?;
                    typed_processes.push(process);
                    branch_types.insert(branch.clone(), typ);
                }

                (
                    process::Command::Match(Arc::clone(branches), Box::from(typed_processes)),
                    Type::Either(span.clone(), branch_types),
                )
            }

            process::Command::Break => {
                self.cannot_have_obligations(span)?;
                (process::Command::Break, Type::Continue(span.clone()))
            }

            process::Command::Continue(process) => {
                let process = self.check_process(process)?;
                (
                    process::Command::Continue(process),
                    Type::Break(span.clone()),
                )
            }

            process::Command::Begin {
                unfounded,
                label,
                captures,
                body: process,
            } => {
                self.loop_points.insert(
                    label.clone(),
                    (subject.clone(), Arc::new(self.variables.clone())),
                );
                let (process, body) = self.infer_process(process, subject)?;
                (
                    process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone(),
                        captures: captures.clone(),
                        body: process,
                    },
                    Type::Recursive {
                        span: span.clone(),
                        asc: if *unfounded {
                            IndexSet::new()
                        } else {
                            IndexSet::from([label.clone()])
                        },
                        label: label.clone(),
                        body: Box::new(body),
                    },
                )
            }

            process::Command::Loop(label, captures) => {
                let Some((driver, variables)) = self.loop_points.get(label).cloned() else {
                    return Err(TypeError::NoSuchLoopPoint(span.clone(), label.clone()));
                };
                if &driver != subject {
                    return Err(TypeError::TypeMustBeKnownAtThisPoint(
                        span.clone(),
                        subject.clone(),
                    ));
                }

                for (var, type_at_begin) in variables.as_ref() {
                    let Some(current_type) = self.get_variable(var) else {
                        return Err(TypeError::LoopVariableNotPreserved(
                            span.clone(),
                            var.clone(),
                        ));
                    };
                    if !current_type.is_assignable_to(
                        type_at_begin,
                        &self.type_defs,
                        &HashSet::new(),
                    )? {
                        return Err(TypeError::LoopVariableChangedType(
                            span.clone(),
                            var.clone(),
                            current_type,
                            type_at_begin.clone(),
                        ));
                    }
                }
                self.cannot_have_obligations(span)?;

                (
                    process::Command::Loop(label.clone(), captures.clone()),
                    Type::Self_(span.clone(), label.clone()),
                )
            }

            process::Command::SendType(_, _) => {
                return Err(TypeError::TypeMustBeKnownAtThisPoint(
                    span.clone(),
                    subject.clone(),
                ))
            }

            process::Command::ReceiveType(parameter, process) => {
                self.type_defs.vars.insert(parameter.clone());
                let (process, then_type) = self.infer_process(process, subject)?;
                (
                    process::Command::ReceiveType(parameter.clone(), process),
                    Type::SendType(span.clone(), parameter.clone(), Box::new(then_type)),
                )
            }
        })
    }

    pub fn check_expression(
        &mut self,
        inference_subject: Option<&Name>,
        expression: &process::Expression<()>,
        target_type: &Type,
    ) -> Result<Arc<process::Expression<Type>>, TypeError> {
        match expression {
            process::Expression::Reference(span, name, ()) => {
                if Some(name) == inference_subject {
                    return Err(TypeError::TypeMustBeKnownAtThisPoint(
                        span.clone(),
                        name.clone(),
                    ));
                }

                let typ = self.get(span, name)?;
                typ.check_assignable(span, target_type, &self.type_defs)?;
                if !typ.is_linear(&self.type_defs)? {
                    self.put(span, name.clone(), typ.clone())?;
                }
                Ok(Arc::new(process::Expression::Reference(
                    span.clone(),
                    name.clone(),
                    typ.clone(),
                )))
            }

            process::Expression::Fork {
                span,
                captures,
                chan_name: channel,
                chan_annotation: annotation,
                process,
                ..
            } => {
                let target_dual = target_type.dual(&self.type_defs)?;
                let (chan_type, expr_type) = match annotation {
                    Some(annotated_type) => {
                        annotated_type.check_assignable(span, &target_dual, &self.type_defs)?;
                        (annotated_type.clone(), target_type) // or annotated_type.dual() ???
                    }
                    None => (target_dual, target_type),
                };
                let mut context = self.split();
                self.capture(inference_subject, captures, &mut context)?;
                context.put(span, channel.clone(), chan_type.clone())?;
                let process = context.check_process(process)?;
                Ok(Arc::new(process::Expression::Fork {
                    span: span.clone(),
                    captures: captures.clone(),
                    chan_name: channel.clone(),
                    chan_annotation: annotation.clone(),
                    chan_type,
                    expr_type: expr_type.clone(),
                    process,
                }))
            }

            process::Expression::Primitive(span, value, ()) => {
                let typ = value.get_type();
                typ.check_assignable(span, target_type, &self.type_defs)?;
                Ok(Arc::new(process::Expression::Primitive(
                    span.clone(),
                    value.clone(),
                    typ,
                )))
            }
        }
    }

    pub fn infer_expression(
        &mut self,
        inference_subject: Option<&Name>,
        expression: &process::Expression<()>,
    ) -> Result<(Arc<process::Expression<Type>>, Type), TypeError> {
        match expression {
            process::Expression::Reference(span, name, ()) => {
                if Some(name) == inference_subject {
                    return Err(TypeError::TypeMustBeKnownAtThisPoint(
                        span.clone(),
                        name.clone(),
                    ));
                }
                let typ = self.get(span, name)?;
                if !typ.is_linear(&self.type_defs)? {
                    self.put(span, name.clone(), typ.clone())?;
                }
                Ok((
                    Arc::new(process::Expression::Reference(
                        span.clone(),
                        name.clone(),
                        typ.clone(),
                    )),
                    typ.clone(),
                ))
            }

            process::Expression::Fork {
                span,
                captures,
                chan_name: channel,
                chan_annotation: annotation,
                process,
                ..
            } => {
                let mut context = self.split();
                self.capture(inference_subject, captures, &mut context)?;
                let (process, typ) = match annotation {
                    Some(typ) => {
                        context.put(span, channel.clone(), typ.clone())?;
                        (context.check_process(process)?, typ.clone())
                    }
                    None => context.infer_process(process, channel)?,
                };
                let dual = typ.dual(&self.type_defs)?;
                Ok((
                    Arc::new(process::Expression::Fork {
                        span: span.clone(),
                        captures: captures.clone(),
                        chan_name: channel.clone(),
                        chan_annotation: annotation.clone(),
                        chan_type: typ,
                        expr_type: dual.clone(),
                        process,
                    }),
                    dual,
                ))
            }

            process::Expression::Primitive(span, value, ()) => {
                let typ = value.get_type();
                Ok((
                    Arc::new(process::Expression::Primitive(
                        span.clone(),
                        value.clone(),
                        typ.clone(),
                    )),
                    typ,
                ))
            }
        }
    }

    pub fn cannot_have_obligations(&mut self, span: &Span) -> Result<(), TypeError> {
        if self.obligations().any(|_| true) {
            return Err(TypeError::UnfulfilledObligations(
                span.clone(),
                self.obligations().cloned().collect(),
            ));
        }
        Ok(())
    }
}

impl Type {
    pub fn pretty(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        match self {
            Self::Primitive(_, PrimitiveType::Int) => write!(f, "Int"),

            Self::Chan(_, body) => {
                write!(f, "chan ")?;
                body.pretty(f, indent)
            }
            Self::Var(_, name) => write!(f, "{}", name),
            Self::Name(_, name, args) => {
                write!(f, "{}", name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        arg.pretty(f, indent)?;
                    }
                    write!(f, ">")?
                }
                Ok(())
            }

            Self::Send(_, arg, then) => {
                let mut then = then;
                write!(f, "(")?;
                arg.pretty(f, indent)?;
                while let Self::Send(_, arg, next) = then.as_ref() {
                    write!(f, ", ")?;
                    arg.pretty(f, indent)?;
                    then = next;
                }
                if let Self::Break(_) = then.as_ref() {
                    write!(f, ")!")
                } else {
                    write!(f, ") ")?;
                    then.pretty(f, indent)
                }
            }

            Self::Receive(_, param, then) => {
                let mut then = then;
                write!(f, "[")?;
                param.pretty(f, indent)?;
                while let Self::Receive(_, param, next) = then.as_ref() {
                    write!(f, ", ")?;
                    param.pretty(f, indent)?;
                    then = next;
                }
                if let Self::Continue(_) = then.as_ref() {
                    write!(f, "]?")
                } else {
                    write!(f, "] ")?;
                    then.pretty(f, indent)
                }
            }

            Self::Either(_, branches) => {
                write!(f, "either {{")?;
                for (branch, typ) in branches {
                    indentation(f, indent + 1)?;
                    write!(f, ".{} ", branch)?;
                    typ.pretty(f, indent + 1)?;
                }
                indentation(f, indent)?;
                write!(f, "}}")
            }

            Self::Choice(_, branches) => {
                write!(f, "{{")?;
                for (branch, typ) in branches {
                    indentation(f, indent + 1)?;
                    write!(f, ".{} => ", branch)?;
                    typ.pretty(f, indent + 1)?;
                }
                indentation(f, indent)?;
                write!(f, "}}")
            }

            Self::Break(_) => write!(f, "!"),
            Self::Continue(_) => write!(f, "?"),

            Self::Recursive { label, body, .. } => {
                write!(f, "recursive ")?;
                if let Some(label) = label {
                    write!(f, ":{} ", label)?;
                }
                body.pretty(f, indent)
            }

            Self::Iterative { label, body, .. } => {
                write!(f, "iterative ")?;
                if let Some(label) = label {
                    write!(f, ":{} ", label)?;
                }
                body.pretty(f, indent)
            }

            Self::Self_(_, label) => {
                write!(f, "self")?;
                if let Some(label) = label {
                    write!(f, " :{}", label)?;
                }
                Ok(())
            }

            Self::SendType(_, name, then) => {
                let mut then = then;
                write!(f, "(type {name}")?;
                while let Self::SendType(_, name, next) = then.as_ref() {
                    write!(f, ", {name}")?;
                    then = next;
                }
                write!(f, ") ")?;
                then.pretty(f, indent)
            }

            Self::ReceiveType(_, name, then) => {
                let mut then = then;
                write!(f, "[type {name}")?;
                while let Self::ReceiveType(_, name, next) = then.as_ref() {
                    write!(f, ", {name}")?;
                    then = next;
                }
                write!(f, "] ")?;
                then.pretty(f, indent)
            }
        }
    }

    pub fn pretty_compact(&self, f: &mut impl Write) -> fmt::Result {
        match self {
            Self::Primitive(_, PrimitiveType::Int) => write!(f, "Int"),

            Self::Chan(_, body) => {
                write!(f, "chan ")?;
                body.pretty_compact(f)
            }
            Self::Var(_, name) => write!(f, "{}", name),
            Self::Name(_, name, args) => {
                write!(f, "{}", name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        arg.pretty_compact(f)?;
                    }
                    write!(f, ">")?
                }
                Ok(())
            }

            Self::Send(_, arg, then) => {
                let mut then = then;
                write!(f, "(")?;
                arg.pretty_compact(f)?;
                while let Self::Send(_, arg, next) = then.as_ref() {
                    write!(f, ", ")?;
                    arg.pretty_compact(f)?;
                    then = next;
                }
                if let Self::Break(_) = then.as_ref() {
                    write!(f, ")!")
                } else {
                    write!(f, ") ")?;
                    then.pretty_compact(f)
                }
            }

            Self::Receive(_, param, then) => {
                let mut then = then;
                write!(f, "[")?;
                param.pretty_compact(f)?;
                while let Self::Receive(_, param, next) = then.as_ref() {
                    write!(f, ", ")?;
                    param.pretty_compact(f)?;
                    then = next;
                }
                if let Self::Continue(_) = then.as_ref() {
                    write!(f, "]?")
                } else {
                    write!(f, "] ")?;
                    then.pretty_compact(f)
                }
            }

            Self::Either(_, branches) => {
                let branches = branches
                    .iter()
                    .map(|(branch, _)| format!(".{branch}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "either {{ {branches} }}")
            }

            Self::Choice(_, branches) => {
                let branches = branches
                    .iter()
                    .map(|(branch, _)| format!(".{branch}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{ {branches} }}")
            }

            Self::Break(_) => write!(f, "!"),
            Self::Continue(_) => write!(f, "?"),

            Self::Recursive { label, body, .. } => {
                write!(f, "recursive ")?;
                if !matches!(body.as_ref(), Self::Either(..)) {
                    if let Some(label) = label {
                        write!(f, ":{} ", label)?;
                    }
                }
                body.pretty_compact(f)
            }

            Self::Iterative { label, body, .. } => {
                write!(f, "iterative ")?;
                if !matches!(body.as_ref(), Self::Choice(..)) {
                    if let Some(label) = label {
                        write!(f, ":{} ", label)?;
                    }
                }
                body.pretty_compact(f)
            }

            Self::Self_(_, label) => {
                write!(f, "self")?;
                if let Some(label) = label {
                    write!(f, " :{}", label)?;
                }
                Ok(())
            }

            Self::SendType(_, name, then) => {
                let mut then = then;
                write!(f, "(type {name}")?;
                while let Self::SendType(_, name, next) = then.as_ref() {
                    write!(f, ", {name}")?;
                    then = next;
                }
                write!(f, ") ")?;
                then.pretty_compact(f)
            }

            Self::ReceiveType(_, name, then) => {
                let mut then = then;
                write!(f, "[type {name}")?;
                while let Self::ReceiveType(_, name, next) = then.as_ref() {
                    write!(f, ", {name}")?;
                    then = next;
                }
                write!(f, "] ")?;
                then.pretty_compact(f)
            }
        }
    }
}

fn indentation(f: &mut impl Write, indent: usize) -> fmt::Result {
    write!(f, "\n")?;
    for _ in 0..indent {
        write!(f, "  ")?;
    }
    Ok(())
}

fn two_labels_from_two_spans(
    code: &str,
    span1: &Span,
    span2: &Span,
    label1: impl Into<Option<String>>,
    label2: impl Into<Option<String>>,
) -> Vec<LabeledSpan> {
    use crate::playground::labels_from_span;
    let mut labels = labels_from_span(code, span1);
    let label1 = label1.into();
    let label2 = label2.into();
    labels.iter_mut().for_each(|x| x.set_label(label1.clone()));
    let mut labels2 = labels_from_span(code, span2);
    labels2.iter_mut().for_each(|x| x.set_label(label2.clone()));
    labels.extend(labels2);
    labels
}

impl TypeError {
    pub fn to_report(&self, source_code: Arc<str>) -> miette::Report {
        use crate::playground::labels_from_span;
        let code = &source_code;
        match self {
            Self::TypeNameAlreadyDefined(span1, span2, name) => {
                miette::miette!(
                    labels = two_labels_from_two_spans(code, span1, span2, "this".to_owned(), "is already defined here".to_owned()),
                    "Type `{}` is already defined.", name
                )
            }
            Self::NameAlreadyDeclared(span1, span2, name) => {
                miette::miette!(
                    labels = two_labels_from_two_spans(code, span1, span2, "this".to_owned(), "is already declared here".to_owned()),
                    "`{}` is already declared.",
                    name,
                )
            }
            Self::NameAlreadyDefined(span1, span2, name) => {
                miette::miette!(
                    labels = two_labels_from_two_spans(code, span1, span2, "this".to_owned(), "is already defined here".to_owned()),
                    "`{}` is already defined",
                    name,
                )
            }
            Self::DeclaredButNotDefined(span,  name) => {
                let mut labels = labels_from_span(code, span);
                labels.iter_mut().for_each(|x| {
                    x.set_label(Some("declared here".to_owned()));
                });
                miette::miette!(
                    labels = labels,
                    "`{}` is declared, but is missing a corresponding definition.",
                    name
                )
            }
            Self::NoMatchingRecursiveOrIterative(span, _) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "This `self` has no matching `recursive` or `iterative`.",
                )
            }
            Self::SelfUsedInNegativePosition(span, _) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "This `self` is used in a negative position.\n\nNegative self-references are not allowed."
                )
            }
            Self::TypeNameNotDefined(span, name) => {
                let labels = labels_from_span(code, span);
                miette::miette!(labels = labels, "Type `{}` is not defined.", name)
            }
            Self::DependencyCycle(span, deps) => {
                let labels = labels_from_span(code, span);
                let mut deps_str = String::new();
                for (i, dep) in deps.iter().enumerate() {
                    if i > 0 {
                        write!(&mut deps_str, " -> ").unwrap();
                    }
                    write!(&mut deps_str, "{}", dep).unwrap();
                }
                miette::miette!(
                    labels = labels,
                    "There is a dependency cycle:\n\n  {}\n\nDependency cycles are not allowed.",
                    deps_str
                )
            }
            Self::WrongNumberOfTypeArgs(span, name, required_number, provided_number) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "Type `{}` has {} type arguments, but {} were provided.",
                    name,
                    required_number,
                    provided_number
                )
            }
            Self::NameNotDefined(span, name) => {
                let labels = labels_from_span(code, span);
                miette::miette!(labels = labels, "`{}` is not defined.", name)
            }
            Self::ShadowedObligation(span, name) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "Cannot re-assign `{}` before handling it.",
                    name,
                )
            }
            Self::TypeMustBeKnownAtThisPoint(span, _) => {
                let labels = labels_from_span(code, span);
                miette::miette!(labels = labels, "Type must be known at this point.")
            }
            Self::ParameterTypeMustBeKnown(span, _, param) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "Type of parameter `{}` must be known.",
                    param,
                )
            }
            Self::CannotAssignFromTo(span, from_type, to_type) => {
                let labels = labels_from_span(code, span);
                let (mut from_type_str, mut to_type_str) = (String::new(), String::new());
                from_type.pretty(&mut from_type_str, 1).unwrap();
                to_type.pretty(&mut to_type_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "This type was required:\n\n  {}\n\nBut an incompatible type was provided:\n\n  {}\n",
                    to_type_str,
                    from_type_str,
                )
            }
            Self::UnfulfilledObligations(span, names) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "Cannot end this process before handling {}.",
                    names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| if i == 0 {
                            format!("`{}`", name)
                        } else {
                            format!(", `{}`", name)
                        })
                        .collect::<String>()
                )
            }
            Self::InvalidOperation(span, _, typ) => {
                let labels = labels_from_span(code, span);
                let mut typ_str = String::new();
                typ.pretty(&mut typ_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "This operation cannot be performed on:\n\n  {}\n",
                    typ_str
                )
            }
            Self::InvalidBranch(span, branch, typ) => {
                let labels = labels_from_span(code, span);
                let mut typ_str = String::new();
                typ.pretty(&mut typ_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "Branch `{}` is not available on:\n\n  {}\n",
                    branch,
                    typ_str
                )
            }
            Self::MissingBranch(span, branch, typ) => {
                let labels = labels_from_span(code, span);
                let mut typ_str = String::new();
                typ.pretty(&mut typ_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "Branch `{}` was not handled for:\n\n  {}\n",
                    branch,
                    typ_str
                )
            }
            Self::RedundantBranch(span, branch, typ) => {
                let labels = labels_from_span(code, span);
                let mut typ_str = String::new();
                typ.pretty(&mut typ_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "Branch `{}` is not possible for:\n\n  {}\n",
                    branch,
                    typ_str
                )
            }
            Self::TypesCannotBeUnified(typ1, typ2) => {
                miette::miette!(
                    labels = two_labels_from_two_spans(
                        code,
                        &typ1.span(),
                        &typ2.span(),
                        "this".to_owned(),
                        "should operate on the same type as this".to_owned()
                    ),
                    "Operations cannot be performed on the same type."
                )
            }
            Self::NoSuchLoopPoint(span, _) => {
                let labels = labels_from_span(code, span);
                miette::miette!(labels = labels, "There is no matching loop point in scope.")
            }
            Self::DoesNotDescendSubjectOfBegin(span, _) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "This `loop` may diverge. Value does not descend from the corresponding `begin`.\n\nIf this is intended, use `unfounded begin`.",
                )
            }
            Self::LoopVariableNotPreserved(span, name) => {
                let labels = labels_from_span(code, span);
                miette::miette!(
                    labels = labels,
                    "`{}` is used by next iteration, but is no longer defined.",
                    name,
                )
            }
            Self::LoopVariableChangedType(span, name, loop_type, begin_type) => {
                let labels = labels_from_span(code, span);
                let (mut loop_type_str, mut begin_type_str) = (String::new(), String::new());
                loop_type.pretty(&mut loop_type_str, 1).unwrap();
                begin_type.pretty(&mut begin_type_str, 1).unwrap();
                miette::miette!(
                    labels = labels,
                    "For next iteration, `{}` is required to be:\n\n  {}\n\nBut it has an incompatible type:\n\n  {}\n",
                    name,
                    begin_type_str,
                    loop_type_str,
                )
            }
            Self::Telltypes(span, variables) => {
                let labels = labels_from_span(code, span);
                let mut buf = String::new();
                for (name, typ) in variables {
                    write!(&mut buf, "{}: ", name).unwrap();
                    typ.pretty(&mut buf, 0).unwrap();
                    write!(&mut buf, "\n\n").unwrap();
                }
                miette::miette! {
                    labels = labels,
                    "{}",
                    buf
                }
            }
        }.with_source_code(source_code)
    }
}

impl TypeError {
    pub fn spans(&self) -> (Span, Option<Span>) {
        match self {
            Self::TypeNameAlreadyDefined(span1, span2, _)
            | Self::NameAlreadyDeclared(span1, span2, _)
            | Self::NameAlreadyDefined(span1, span2, _) => (span1.clone(), Some(span2.clone())),

            Self::DeclaredButNotDefined(span, _)
            | Self::NoMatchingRecursiveOrIterative(span, _)
            | Self::SelfUsedInNegativePosition(span, _)
            | Self::TypeNameNotDefined(span, _)
            | Self::DependencyCycle(span, _)
            | Self::WrongNumberOfTypeArgs(span, _, _, _)
            | Self::NameNotDefined(span, _)
            | Self::ShadowedObligation(span, _)
            | Self::TypeMustBeKnownAtThisPoint(span, _)
            | Self::ParameterTypeMustBeKnown(span, _, _)
            | Self::CannotAssignFromTo(span, _, _)
            | Self::UnfulfilledObligations(span, _)
            | Self::InvalidOperation(span, _, _)
            | Self::InvalidBranch(span, _, _)
            | Self::MissingBranch(span, _, _)
            | Self::RedundantBranch(span, _, _)
            | Self::NoSuchLoopPoint(span, _)
            | Self::DoesNotDescendSubjectOfBegin(span, _)
            | Self::LoopVariableNotPreserved(span, _)
            | Self::LoopVariableChangedType(span, _, _, _)
            | Self::Telltypes(span, _) => (span.clone(), None),

            Self::TypesCannotBeUnified(typ1, typ2) => (typ1.span(), Some(typ2.span())),
        }
    }
}
