use core::future::Future;

/// Workaround for calling async function callback with lifetime parameter
/// Source:https://www.reddit.com/r/rust/comments/hey4oa/comment/fvv1zql/
pub trait AsyncCallback<'a, T> {
    type Output: 'a + Future<Output = ()>;
    fn call(&self, argument: &'a T) -> Self::Output;
}

impl<'a, R: 'a, F, T: 'a> AsyncCallback<'a, T> for F
where
    F: Fn(&'a T) -> R,
    R: Future<Output = ()> + 'a,
{
    type Output = R;
    fn call(&self, argument: &'a T) -> Self::Output {
        self(argument)
    }
}
