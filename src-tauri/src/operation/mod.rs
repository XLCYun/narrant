mod cancellable;

#[cfg(test)]
pub(crate) use cancellable::TestCancellation;
pub(crate) use cancellable::{
    CancellableOperations, CancellationSignal, OperationContext, OperationError, OperationId,
};
