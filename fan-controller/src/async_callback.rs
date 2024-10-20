use core::future::Future;

/// Workaround for calling async function callback with lifetime parameter
/// Source:https://www.reddit.com/r/rust/comments/hey4oa/comment/fvv1zql/
pub trait AsyncCallback<'a> {
    type Output: 'a + Future<Output = ()>;
    fn call(&self, argument1: &'a str, argument2: &'a [u8]) -> Self::Output;
}

impl<'a, R: 'a, F> AsyncCallback<'a> for F
where
    F: Fn(&'a  str, &'a [u8]) -> R,
    R: Future<Output = ()> + 'a,
{
    type Output = R;
    fn call(&self, argument1: &'a str, argument2: &'a [u8]) -> Self::Output {
        self(argument1, argument2)
    }
}