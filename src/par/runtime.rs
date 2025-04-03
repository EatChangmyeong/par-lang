use futures::{
    channel::oneshot,
    task::{Spawn, SpawnExt},
};
use indexmap::IndexMap;
use std::{hash::Hash, sync::Arc};
use crate::location::Span;
use super::process::{Captures, Command, Expression, Process};

#[derive(Clone, Debug)]
pub enum Error<Name> {
    NameNotDefined(Span, Name),
    ShadowedObligation(Span, Name),
    UnfulfilledObligations(Span, Vec<Name>),
    IncompatibleOperations(Operation<Name>, Operation<Name>),
    NoSuchLoopPoint(Span, Option<Name>),
    Multiple(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum Operation<Name> {
    Unknown(Span),
    Send(Span),
    Receive(Span),
    Choose(Span, Name),
    Match(Span, Arc<[Name]>),
    Break(Span),
    Continue(Span),
}

pub enum Message<Name> {
    Swap(Request<Name>, oneshot::Sender<Self>),
    Send(Span, Value<Name>, oneshot::Receiver<Self>),
    Choose(Span, Name, oneshot::Receiver<Self>),
    Break(Span),
    Error(Error<Name>),
}

impl<Name> Message<Name> {
    pub fn into_operation_and_values(
        self,
    ) -> Result<(Operation<Name>, Vec<Value<Name>>), Error<Name>> {
        match self {
            Message::Swap(request, tx) => Ok((request.into_operation(), vec![Value::Sender(tx)])),
            Message::Send(loc, value, rx) => {
                Ok((Operation::Send(loc), vec![value, Value::Receiver(rx)]))
            }
            Message::Choose(loc, chosen, rx) => {
                Ok((Operation::Choose(loc, chosen), vec![Value::Receiver(rx)]))
            }
            Message::Break(loc) => Ok((Operation::Break(loc), vec![])),
            Message::Error(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Request<Name> {
    Receive(Span),
    Match(Span, Arc<[Name]>),
    Continue(Span),
    Dynamic(Span),
}

impl<Name> Request<Name> {
    pub fn into_operation(self) -> Operation<Name> {
        match self {
            Request::Receive(loc) => Operation::Receive(loc),
            Request::Match(loc, choices) => Operation::Match(loc, choices),
            Request::Continue(loc) => Operation::Continue(loc),
            Request::Dynamic(loc) => Operation::Unknown(loc),
        }
    }
}

impl<Name> Request<Name> {
    pub fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Dynamic(_), _) | (_, Self::Dynamic(_)) => true,
            (Self::Receive(_), Self::Receive(_)) => true,
            (Self::Match(_, _), Self::Match(_, _)) => true,
            (Self::Continue(_), Self::Continue(_)) => true,
            (_, _) => false,
        }
    }
}

pub enum Value<Name> {
    Receiver(oneshot::Receiver<Message<Name>>),
    Sender(oneshot::Sender<Message<Name>>),
}

pub struct Context<Name, Typ> {
    spawner: Arc<dyn Spawn + Send + Sync>,
    globals: Arc<IndexMap<Name, Arc<Expression<Name, Typ>>>>,
    variables: IndexMap<Name, Value<Name>>,
    loop_points: IndexMap<Option<Name>, (Name, Arc<Process<Name, Typ>>)>,
}

impl<Name, Typ> Context<Name, Typ>
where
    Name: Clone + Eq + Hash + Send + Sync + 'static,
    Typ: Send + Sync + 'static,
{
    pub fn new(
        spawner: Arc<dyn Spawn + Send + Sync>,
        globals: Arc<IndexMap<Name, Arc<Expression<Name, Typ>>>>,
    ) -> Self {
        Self {
            spawner,
            globals,
            variables: IndexMap::new(),
            loop_points: IndexMap::new(),
        }
    }

    pub fn spawner(&self) -> Arc<dyn Spawn + Send + Sync> {
        Arc::clone(&self.spawner)
    }

    pub fn split(&self) -> Self {
        Self {
            spawner: Arc::clone(&self.spawner),
            globals: Arc::clone(&self.globals),
            variables: IndexMap::new(),
            loop_points: self.loop_points.clone(),
        }
    }

    pub fn get_variable(&mut self, name: &Name) -> Option<Value<Name>> {
        self.variables.shift_remove(name)
    }

    pub fn get(&mut self, loc: &Span, name: &Name) -> Result<Value<Name>, Error<Name>> {
        match self.get_variable(name) {
            Some(value) => Ok(value),
            None => match self.globals.get(name) {
                Some(expression) => self.evaluate(&Arc::clone(expression)),
                None => self.throw([], Error::NameNotDefined(loc.clone(), name.clone())),
            },
        }
    }

    pub fn put(
        &mut self,
        loc: &Span,
        name: Name,
        value: Value<Name>,
    ) -> Result<(), Error<Name>> {
        if let Some(value) = self.variables.shift_remove(&name) {
            return self.throw([value], Error::ShadowedObligation(loc.clone(), name));
        }
        self.variables.insert(name, value);
        Ok(())
    }

    pub fn capture(
        &mut self,
        cap: &Captures<Name>,
        target: &mut Self,
    ) -> Result<(), Error<Name>> {
        for (name, loc) in &cap.names {
            let value = match self.get_variable(name) {
                Some(value) => value,
                None => continue,
            };
            target.put(loc, name.clone(), value)?;
        }
        Ok(())
    }

    pub fn obligations(&self) -> impl Iterator<Item = &Name> {
        self.variables.iter().map(|(name, _)| name)
    }

    pub fn evaluate(
        &mut self,
        expression: &Expression<Name, Typ>,
    ) -> Result<Value<Name>, Error<Name>> {
        match expression {
            Expression::Reference(loc, name, _) => self.get(loc, name),

            Expression::Fork { span, captures: cap, chan_name: channel, process, .. } => {
                let mut context = self.split();
                self.capture(cap, &mut context)?;

                let (tx, rx) = oneshot::channel();
                context.put(span, channel.clone(), Value::Sender(tx))?;

                let process = Arc::clone(process);
                self.spawner
                    .spawn(async move {
                        let _ = context.run(process).await;
                    })
                    .expect("could not spawn");

                Ok(Value::Receiver(rx))
            }
        }
    }

    pub async fn run(
        &mut self,
        process: Arc<Process<Name, Typ>>,
    ) -> Result<(), Error<Name>> {
        let mut current_process = process;
        loop {
            match current_process.as_ref() {
                Process::Let { span: loc, name, annotation: _, typ: _, value: expression, then: process } => {
                    let value = self.evaluate(expression)?;
                    self.put(loc, name.clone(), value)?;
                    current_process = Arc::clone(process);
                }

                Process::Do { span: loc, name: object_name, typ: _, command } => {
                    let object = self.get(loc, object_name)?;

                    match command {
                        Command::Link(expression) => {
                            let value = match self.evaluate(&expression) {
                                Ok(value) => value,
                                Err(error) => return self.throw([object], error),
                            };
                            return self.link(loc.clone(), object, value).await;
                        }

                        Command::Send(argument, process) => {
                            let argument = match self.evaluate(&argument) {
                                Ok(value) => value,
                                Err(error) => return self.throw([object], error),
                            };
                            let object = self.send_to(loc.clone(), object, argument).await?;
                            self.put(loc, object_name.clone(), object)?;
                            current_process = Arc::clone(process);
                        }

                        Command::Receive(parameter, _, process) => {
                            let (argument, object) = self.receive_from(loc.clone(), object).await?;
                            self.put(loc, object_name.clone(), object)?;
                            self.put(loc, parameter.clone(), argument)?;
                            current_process = Arc::clone(process);
                        }

                        Command::Choose(chosen, process) => {
                            let object =
                                self.choose_in(loc.clone(), object, chosen.clone()).await?;
                            self.put(loc, object_name.clone(), object)?;
                            current_process = Arc::clone(process);
                        }

                        Command::Match(choices, processes) => {
                            let (loc1, chosen, object) = self
                                .either_of(loc.clone(), object, Arc::clone(choices))
                                .await?;
                            let index = match choices.iter().position(|c| c == &chosen) {
                                Some(index) => index,
                                None => {
                                    return self.throw(
                                        [object],
                                        Error::IncompatibleOperations(
                                            Operation::Choose(loc1, chosen),
                                            Operation::Match(loc.clone(), Arc::clone(choices)),
                                        ),
                                    )
                                }
                            };
                            self.put(loc, object_name.clone(), object)?;
                            current_process = Arc::clone(&processes[index]);
                        }

                        Command::Break => {
                            return self.break_to(loc.clone(), object).await;
                        }

                        Command::Continue(process) => {
                            self.continue_from(loc.clone(), object).await?;
                            current_process = Arc::clone(process);
                        }

                        Command::Begin { unfounded: _, label: point, body: process } => {
                            self.loop_points
                                .insert(point.clone(), (object_name.clone(), Arc::clone(process)));
                            self.put(loc, object_name.clone(), object)?;
                            current_process = Arc::clone(process);
                        }

                        Command::Loop(point) => {
                            let Some((name, process)) = self.loop_points.get(point) else {
                                return self.throw(
                                    [object],
                                    Error::NoSuchLoopPoint(loc.clone(), point.clone()),
                                );
                            };
                            let name = name.clone();
                            let process = Arc::clone(process);
                            self.put(loc, name, object)?;
                            current_process = process;
                        }

                        Command::SendType(_, process) | Command::ReceiveType(_, process) => {
                            self.put(loc, object_name.clone(), object)?;
                            current_process = Arc::clone(process);
                        }
                    }
                }

                Process::Telltypes(_, process) => {
                    current_process = Arc::clone(process);
                }
            }
        }
    }

    pub async fn link(
        &mut self,
        loc: Span,
        left: Value<Name>,
        right: Value<Name>,
    ) -> Result<(), Error<Name>> {
        let [left, right] = self.cannot_have_obligations(&loc, [left, right]).await?;
        match (left, right) {
            (Value::Receiver(rx1), Value::Receiver(rx2)) => {
                match (
                    rx1.await.ok().expect("sender dropped"),
                    rx2.await.ok().expect("sender dropped"),
                ) {
                    (Message::Swap(_, tx), message) | (message, Message::Swap(_, tx)) => {
                        tx.send(message).ok().expect("receiver dropped");
                    }
                    (message1, message2) => self.invalid_message_and_message(message1, message2)?,
                }
            }
            (Value::Sender(tx1), Value::Sender(tx2)) => {
                let message = self
                    .swap(Request::Dynamic(loc), tx1)
                    .await
                    .ok()
                    .expect("sender dropped");
                tx2.send(message).ok().expect("receiver dropped");
            }
            (Value::Receiver(rx), Value::Sender(tx)) | (Value::Sender(tx), Value::Receiver(rx)) => {
                let message = rx.await.ok().expect("sender dropped");
                tx.send(message).ok().expect("receiver dropped");
            }
        }
        Ok(())
    }

    pub async fn send_to(
        &mut self,
        loc: Span,
        object: Value<Name>,
        argument: Value<Name>,
    ) -> Result<Value<Name>, Error<Name>> {
        let tx = match object {
            Value::Receiver(rx) => self.expect_swap(Request::Receive(loc.clone()), rx).await?,
            Value::Sender(tx) => tx,
        };
        let (tx1, rx1) = oneshot::channel();
        tx.send(Message::Send(loc, argument, rx1))
            .ok()
            .expect("receiver dropped");
        Ok(Value::Sender(tx1))
    }

    pub async fn receive_from(
        &mut self,
        loc: Span,
        object: Value<Name>,
    ) -> Result<(Value<Name>, Value<Name>), Error<Name>> {
        let mut rx = match object {
            Value::Receiver(rx) => rx,
            Value::Sender(tx) => self.swap(Request::Receive(loc.clone()), tx),
        };
        loop {
            match rx.await.ok().expect("sender dropped") {
                Message::Swap(Request::Dynamic(_), tx) => {
                    rx = self.swap(Request::Receive(loc.clone()), tx);
                    continue;
                }
                Message::Send(_, argument, rx) => return Ok((argument, Value::Receiver(rx))),
                message => return self.invalid_message_and_request(message, Request::Receive(loc)),
            }
        }
    }

    pub async fn choose_in(
        &mut self,
        loc: Span,
        object: Value<Name>,
        chosen: Name,
    ) -> Result<Value<Name>, Error<Name>> {
        let tx = match object {
            Value::Receiver(rx) => self.expect_swap_choose(loc.clone(), &chosen, rx).await?,
            Value::Sender(tx) => tx,
        };
        let (tx1, rx1) = oneshot::channel();
        tx.send(Message::Choose(loc, chosen, rx1))
            .ok()
            .expect("receiver dropped");
        Ok(Value::Sender(tx1))
    }

    pub async fn either_of(
        &mut self,
        loc: Span,
        object: Value<Name>,
        choices: Arc<[Name]>,
    ) -> Result<(Span, Name, Value<Name>), Error<Name>> {
        let request = Request::Match(loc.clone(), Arc::clone(&choices));
        let mut rx = match object {
            Value::Receiver(rx) => rx,
            Value::Sender(tx) => self.swap(request.clone(), tx),
        };
        loop {
            match rx.await.ok().expect("sender dropped") {
                Message::Swap(Request::Dynamic(_), tx) => {
                    rx = self.swap(request.clone(), tx);
                    continue;
                }
                Message::Choose(loc, chosen, rx) => return Ok((loc, chosen, Value::Receiver(rx))),
                message => return self.invalid_message_and_request(message, request),
            }
        }
    }

    pub async fn break_to(
        &mut self,
        loc: Span,
        object: Value<Name>,
    ) -> Result<(), Error<Name>> {
        let [object] = self.cannot_have_obligations(&loc, [object]).await?;
        let tx = match object {
            Value::Receiver(rx) => self.expect_swap(Request::Continue(loc.clone()), rx).await?,
            Value::Sender(tx) => tx,
        };
        tx.send(Message::Break(loc)).ok().expect("receiver dropped");
        Ok(())
    }

    pub async fn continue_from(
        &mut self,
        loc: Span,
        object: Value<Name>,
    ) -> Result<(), Error<Name>> {
        let mut rx = match object {
            Value::Receiver(rx) => rx,
            Value::Sender(tx) => self.swap(Request::Continue(loc.clone()), tx),
        };
        loop {
            match rx.await.ok().expect("sender dropped") {
                Message::Swap(Request::Dynamic(_), tx) => {
                    rx = self.swap(Request::Continue(loc.clone()), tx);
                    continue;
                }
                Message::Break(_) => return Ok(()),
                message => {
                    return self.invalid_message_and_request(message, Request::Continue(loc))
                }
            }
        }
    }

    async fn cannot_have_obligations<V: IntoIterator<Item = Value<Name>>>(
        &mut self,
        loc: &Span,
        values: V,
    ) -> Result<V, Error<Name>> {
        if self.obligations().any(|_| true) {
            return self.throw(
                values,
                Error::UnfulfilledObligations(loc.clone(), self.obligations().cloned().collect()),
            );
        }
        Ok(values)
    }

    pub fn swap(
        &mut self,
        request: Request<Name>,
        tx: oneshot::Sender<Message<Name>>,
    ) -> oneshot::Receiver<Message<Name>> {
        let (tx1, rx1) = oneshot::channel();
        tx.send(Message::Swap(request, tx1))
            .ok()
            .expect("receiver dropped");
        rx1
    }

    async fn expect_swap(
        &mut self,
        expected_request: Request<Name>,
        rx: oneshot::Receiver<Message<Name>>,
    ) -> Result<oneshot::Sender<Message<Name>>, Error<Name>> {
        match rx.await.ok().expect("sender dropped") {
            Message::Swap(request, tx) if request.matches(&expected_request) => Ok(tx),
            message => self.invalid_message_and_request(message, expected_request),
        }
    }

    async fn expect_swap_choose(
        &mut self,
        loc: Span,
        chosen: &Name,
        rx: oneshot::Receiver<Message<Name>>,
    ) -> Result<oneshot::Sender<Message<Name>>, Error<Name>> {
        match rx.await.ok().expect("sender dropped") {
            Message::Swap(Request::Dynamic(_), tx) => Ok(tx),
            Message::Swap(Request::Match(_, choices), tx)
                if choices.iter().any(|c| c == chosen) =>
            {
                Ok(tx)
            }
            message => self.invalid_message_and_request(
                message,
                Request::Match(loc, Arc::new([chosen.clone()])),
            ),
        }
    }

    fn invalid_message_and_request<T>(
        &mut self,
        message: Message<Name>,
        request: Request<Name>,
    ) -> Result<T, Error<Name>> {
        match (
            message.into_operation_and_values(),
            request.into_operation(),
        ) {
            (Err(error), _) => self.throw([], error),
            (Ok((op1, values)), op2) => self.throw(values, Error::IncompatibleOperations(op1, op2)),
        }
    }

    fn invalid_message_and_message<T>(
        &mut self,
        message1: Message<Name>,
        message2: Message<Name>,
    ) -> Result<T, Error<Name>> {
        match (
            message1.into_operation_and_values(),
            message2.into_operation_and_values(),
        ) {
            (Err(error1), Err(error2)) => {
                self.throw([], Error::Multiple(Box::new(error1), Box::new(error2)))
            }
            (Err(error), Ok((_, values))) | (Ok((_, values)), Err(error)) => {
                self.throw(values, error)
            }
            (Ok((op1, values1)), Ok((op2, values2))) => self.throw(
                values1.into_iter().chain(values2),
                Error::IncompatibleOperations(op1, op2),
            ),
        }
    }

    fn throw<T>(
        &mut self,
        values: impl IntoIterator<Item = Value<Name>>,
        error: Error<Name>,
    ) -> Result<T, Error<Name>> {
        let mut pending = self
            .variables
            .drain(..)
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        pending.extend(values);

        self.spawner
            .spawn({
                let mut error = error.clone();
                async move {
                    while let Some(value) = pending.pop() {
                        match value {
                            Value::Receiver(rx) => match rx.await.ok().expect("sender dropped") {
                                Message::Swap(_, tx) => pending.push(Value::Sender(tx)),
                                Message::Send(_, argument, rx) => {
                                    pending.push(argument);
                                    pending.push(Value::Receiver(rx));
                                }
                                Message::Choose(_, _, rx) => pending.push(Value::Receiver(rx)),
                                Message::Break(_) => (),
                                Message::Error(error1) => {
                                    error = Error::Multiple(Box::new(error), Box::new(error1))
                                }
                            },
                            Value::Sender(tx) => tx
                                .send(Message::Error(error.clone()))
                                .ok()
                                .expect("receiver dropped"),
                        }
                    }
                }
            })
            .expect("spawn failed");

        Err(error)
    }
}
