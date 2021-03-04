use futures::future::FusedFuture;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper::{Body, Response, StatusCode};
use log::{trace, warn};

use crate::handler::IntoResponse;
use crate::helpers::http::response::create_empty_response;
use crate::state::{request_id, State};

/// Describes an error which occurred during handler execution, and allows the creation of a HTTP
/// `Response`.
#[derive(Debug)]
pub struct HandlerError {
    status_code: StatusCode,
    cause: anyhow::Error,
    // Customize the response body when error occurs, when it is not `None`, it will be served as response.
    // This field is set by
    // * method: set_customized_response_body
    //   fn set_customized_response_body<F: FnOnce(&State) -> R, R: IntoResponse>(&mut self, state: &State, f: F)
    // or by method of trait (MapHandlerErrorToCustomizedResponse):
    //   fn map_err_to_response<F: FnOnce(&State) -> R, R: IntoResponse>(self, state: &State, f: F) -> Result<T, HandlerError>
    customized_response_body: Option<Response<Body>>,
}

/// Convert a generic `anyhow::Error` into a `HandlerError`, similar as you would a concrete error
/// type with `into_handler_error()`.
impl<E> From<E> for HandlerError
where
    E: Into<anyhow::Error> + Display,
{
    fn from(error: E) -> HandlerError {
        trace!(" converting Error to HandlerError: {}", error);

        HandlerError {
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            cause: error.into(),
            customized_response_body: None,
        }
    }
}

// pub trait CusTrait<T>{
//     fn cus_trait(&self);
// }

// //
// impl<E> From<E> for (State, HandlerError)
//     where
//         E: Into<anyhow::Error> + Display,
// {
//     fn from(error: E) -> (State, HandlerError) {
//         trace!(" converting Error to HandlerError: {}", error);
//
//         (State::new(),HandlerError::from(error))
//     }
// }

impl HandlerError {
    /// Returns the HTTP status code associated with this `HandlerError`.
    pub fn status(&self) -> StatusCode {
        self.status_code
    }

    /// Customize the response body when error occurs, when it is not `None`, it will be served as response.
    pub fn set_customized_response_body<F: FnOnce(&State) -> R, R: IntoResponse>(
        &mut self,
        state: &State,
        f: F,
    ) {
        let body = f(state).into_response(state);
        self.status_code = body.status(); // update status_code by the customized response.
        self.customized_response_body = Some(body);
        // self
    }

    // pub fn map_customized_response_body<F: FnOnce(E, &State) -> R, R: IntoResponse, E: Into<anyhow::Error> + Display>(&mut self, err: E, state: &State, f: F) {
    //     let body = f(err, state).into_response(state);
    //     self.status_code = body.status(); // update status_code by the customized response.
    //     self.customized_response_body = Some(body);
    //     // self
    // }

    /// Sets the HTTP status code of the response which is generated by the `IntoResponse`
    /// implementation.
    ///
    /// ```rust
    /// # extern crate gotham;
    /// # extern crate hyper;
    /// # extern crate futures;
    /// #
    /// # use std::pin::Pin;
    /// #
    /// # use futures::prelude::*;
    /// # use hyper::StatusCode;
    /// # use gotham::state::State;
    /// # use gotham::handler::{HandlerError, HandlerFuture};
    /// # use gotham::test::TestServer;
    /// #
    /// fn handler(state: State) -> Pin<Box<HandlerFuture>> {
    ///     // It's OK if this is bogus, we just need something to convert into a `HandlerError`.
    ///     let io_error = std::io::Error::last_os_error();
    ///
    ///     let handler_error = HandlerError::from(io_error)
    ///         .with_status(StatusCode::IM_A_TEAPOT);
    ///
    ///     future::err((state, handler_error)).boxed()
    /// }
    ///
    /// # fn main() {
    /// #
    /// let test_server = TestServer::new(|| Ok(handler)).unwrap();
    /// let response = test_server.client().get("http://example.com/").perform().unwrap();
    /// assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
    /// #
    /// # }
    /// ```
    pub fn with_status(self, status_code: StatusCode) -> HandlerError {
        HandlerError {
            status_code,
            ..self
        }
    }

    /// Attempt to downcast the cause by reference.
    pub fn downcast_cause_ref<E>(&self) -> Option<&E>
    where
        E: Display + Debug + Send + Sync + 'static,
    {
        self.cause.downcast_ref()
    }

    /// Attempt to downcast the cause by mutable reference.
    pub fn downcast_cause_mut<E>(&mut self) -> Option<&mut E>
    where
        E: Display + Debug + Send + Sync + 'static,
    {
        self.cause.downcast_mut()
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self, state: &State) -> Response<Body> {
        warn!(
            "[{}] HandlerError is generating {} {} response: {}",
            request_id(state),
            self.status_code.as_u16(),
            self.status_code
                .canonical_reason()
                .unwrap_or("(unregistered)"),
            self.cause
        );

        if let Some(rsp) = self.customized_response_body {
            rsp
        } else {
            create_empty_response(state, self.status_code)
        }
    }
}

/// This trait allows you to convert a `Result`'s `Err` case into a handler error with the given
/// status code. This is handy if you want to specify the status code but still use the `?`
/// shorthand.
///
/// ```rust
/// # extern crate gotham;
/// # use gotham::anyhow::anyhow;
/// # use gotham::handler::{HandlerError, MapHandlerError};
/// # use gotham::hyper::StatusCode;
/// fn handler() -> Result<(), HandlerError> {
/// 	let result = Err(anyhow!("just a test"));
/// 	result.map_err_with_status(StatusCode::IM_A_TEAPOT)?;
/// 	unreachable!()
/// }
///
/// # #[allow(non_snake_case)]
/// # fn Err<T>(err: T) -> Result<(), T> {
/// #   Result::Err(err)
/// # }
/// # fn main() {
/// let response = handler();
/// assert_eq!(response.map_err(|err| err.status()), Err(StatusCode::IM_A_TEAPOT));
/// # }
/// ```
pub trait MapHandlerError<T> {
    /// Equivalent of `map_err(|err| HandlerError::from(err).with_status(status_code))`.
    fn map_err_with_status(self, status_code: StatusCode) -> Result<T, HandlerError>;
}

impl<T, E> MapHandlerError<T> for Result<T, E>
where
    E: Into<anyhow::Error> + Display,
{
    fn map_err_with_status(self, status_code: StatusCode) -> Result<T, HandlerError> {
        self.map_err(|err| {
            trace!(" converting Error to HandlerError: {}", err);
            HandlerError {
                status_code,
                cause: err.into(),
                customized_response_body: None,
            }
        })
    }
}

/// more concrete version of Result<T,E> with E=handlerError
impl<T> MapHandlerError<T> for Result<T, HandlerError> {
    fn map_err_with_status(self, status_code: StatusCode) -> Result<T, HandlerError> {
        self.map_err(|mut err| {
            trace!(" converting Error to HandlerError: {:?}", err);
            err.status_code = status_code;
            err
        })
    }
}

/// # customize response for HandlerError
/// ## Why do we need it?
/// We might want to customize different response for different error, eg:
/// * for authorized-user-resource, we might return 403(Forbidden) for an un-logged-in user;
/// * or when some file user requesting is not found, we might return 404;
/// * ...
/// Or we just want to send with content-type: application/json when request is application/json.
/// (In the old, we just send 500 with plain/text for any request if error happens)
/// ## How to use it?
/// Here is a very simple demo:
/// ```no-compile
/// // error response will return json or plain by request content-type.
/// pub async fn map_err_to_customized_response(
///     state: &mut State,
/// ) -> Result<impl IntoResponse, HandlerError> {
///     // here, we just simulate an err.
///     let _io_error = Err(std::io::Error::last_os_error()).map_err_to_customized_response(
///         state,
///         |err, state| {
///             // print the error
///             println!("error occurs: {}", err);
///             let content_type = HeaderMap::borrow_from(&state)
///                 .get(CONTENT_TYPE)
///                 .map(|x| x.to_str().unwrap())
///                 .unwrap_or("text/plain");
///             if content_type.contains("json") {
///                 // an error occurs, but still we want to send OK to client
///                 let customized_response = (
///                     StatusCode::SERVICE_UNAVAILABLE,
///                     mime::APPLICATION_JSON,
///                     r##" {"customized_error_to_return_json_response": "yes", "last_os_error": "##
///                         .to_owned()
///                         + &format!("{:?}", err.to_string())
///                         + "}  ",
///                 );
///                 (err, customized_response)
///             } else {
///                 let customized_response = (
///                     StatusCode::SERVICE_UNAVAILABLE,
///                     mime::TEXT_PLAIN_UTF_8,
///                     format!(
///                         "customized_error_to_return_json_response: yes, last_os_error: {}",
///                         err
///                     ),
///                 );
///                 (err, customized_response)
///             }
///         },
///     )?;
///     Ok(create_empty_response(&state, StatusCode::OK))
/// }
/// ```
pub trait MapHandlerErrorToCustomizedResponse<T, E>
where
    E: Into<anyhow::Error> + Display,
{
    /// Why choose:
    ///  * choose: fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, state: &State, f: F) -> Result<T, HandlerError>;
    ///  * instead of: fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, status_code: StatusCode, state: &State, f: F) -> Result<T, HandlerError>; ?
    /// Because we customized the response via F and remember, a response already includes status_code, so we don't need it.
    /// Ofcourse We can always set status_code in customized response if needed.
    // fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, status_code: StatusCode, state: &State, f: F) -> Result<T, HandlerError>;
    fn map_err_to_customized_response<F: FnOnce(E, &State) -> (E, R), R: IntoResponse>(
        self,
        state: &State,
        f: F,
    ) -> Result<T, HandlerError>;
}

impl<T, E> MapHandlerErrorToCustomizedResponse<T, E> for Result<T, E>
where
    E: Into<anyhow::Error> + Display,
{
    fn map_err_to_customized_response<F: FnOnce(E, &State) -> (E, R), R: IntoResponse>(
        self,
        state: &State,
        f: F,
    ) -> Result<T, HandlerError> {
        self.map_err(|err| {
            trace!(" map_err_to_customized_response by error: {}", err);
            // let rsp = f(state).into_response(state);
            // let mut e = HandlerError::from(err);

            let (e, body) = f(err, state);
            let mut handler_error = HandlerError::from(e);
            let rsp = body.into_response(state);
            handler_error.status_code = rsp.status(); // update status_code by the customized response.
            handler_error.customized_response_body = Some(rsp);
            handler_error
        })
    }
}

/// # customize response for HandlerError
/// ## Why do we need it?
/// We might want to customize different response for different error, eg:
/// * for authorized-user-resource, we might return 403(Forbidden) for an un-logged-in user;
/// * or when some file user requesting is not found, we might return 404;
/// * ...
/// Or we just want to send with content-type: application/json when request is application/json.
/// (In the old, we just send 500 with plain/text for any request if error happens)
/// ## How to use it?
/// Here is a very simple demo:
/// ```no-compile
/// pub async fn map_err_with_customized_response(
///     state: &mut State,
/// ) -> Result<impl IntoResponse, HandlerError> {
///     // here, we just simulate an err.
///     let _io_error = Err(std::io::Error::last_os_error())
///         .map_err_with_customized_response(
///             state,
///             |_state| {
///                 // an error occurs, but still sending **OK** to client
///                 (StatusCode::OK, mime::TEXT_PLAIN_UTF_8, "Customized response by the last os error (Intentionally return 200 even error occurs)")
///             },
///         )?;
///     Ok(create_empty_response(&state, StatusCode::OK))
/// }
///
pub trait MapHandlerErrorWithCustomizedResponse<T> {
    /// Why choose:
    ///  * choose: fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, state: &State, f: F) -> Result<T, HandlerError>;
    ///  * instead of: fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, status_code: StatusCode, state: &State, f: F) -> Result<T, HandlerError>; ?
    /// Because we customized the response via F and remember, a response already includes status_code, so we don't need it.
    /// Ofcourse We can always set status_code in customized response if needed.
    // fn map_err_to_response<F: FnOnce(&State) -> Response<Body>>(self, status_code: StatusCode, state: &State, f: F) -> Result<T, HandlerError>;
    fn map_err_with_customized_response<F: FnOnce(&State) -> R, R: IntoResponse>(
        self,
        state: &State,
        f: F,
    ) -> Result<T, HandlerError>;
}

impl<T, E> MapHandlerErrorWithCustomizedResponse<T> for Result<T, E>
where
    E: Into<anyhow::Error> + Display,
{
    fn map_err_with_customized_response<F: FnOnce(&State) -> R, R: IntoResponse>(
        self,
        state: &State,
        f: F,
    ) -> Result<T, HandlerError> {
        self.map_err(|err| {
            trace!(" map_err_with_customized_response by error: {}", err);
            // let rsp = f(state).into_response(state);
            let mut e = HandlerError::from(err);
            e.set_customized_response_body(state, f);
            e
        })
    }
}
// impl<T> MapHandlerErrorToResponse<T> for Result<T, HandlerError>
// {
//     fn map_err_to_response<F: FnOnce(&State) -> R, R: IntoResponse>(self, state: &State, f: F) -> Result<T, HandlerError> {
//         self.map_err(|mut err| {
//             trace!(" converting Error to HandlerError: {:?}", err);
//             err.cus_response_body = Option::from(f(state).into_response(state));
//             err
//         })
//     }
// }

// The future for `map_err_with_status`.
#[pin_project::pin_project(project = MapErrWithStatusProj, project_replace = MapErrWithStatusProjOwn)]
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub enum MapErrWithStatus<F> {
    Incomplete {
        #[pin]
        future: F,
        status: StatusCode,
    },
    Complete,
}

impl<F> MapErrWithStatus<F> {
    fn new(future: F, status: StatusCode) -> Self {
        Self::Incomplete { future, status }
    }
}

impl<F, T, E> FusedFuture for MapErrWithStatus<F>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error> + Display,
{
    fn is_terminated(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

impl<F, T, E> Future for MapErrWithStatus<F>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error> + Display,
{
    type Output = Result<T, HandlerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().project() {
            MapErrWithStatusProj::Incomplete { future, .. } => {
                let output = match future.poll(cx) {
                    Poll::Ready(output) => output,
                    Poll::Pending => return Poll::Pending,
                };
                match self.project_replace(MapErrWithStatus::Complete) {
                    MapErrWithStatusProjOwn::Incomplete { status, .. } => {
                        Poll::Ready(output.map_err_with_status(status))
                    }
                    MapErrWithStatusProjOwn::Complete => unreachable!(),
                }
            }
            MapErrWithStatusProj::Complete => {
                panic!("MapErrWithStatus must not be polled after it returned `Poll::Ready`")
            }
        }
    }
}

/// This trait allows you to convert a `Result`'s `Err` case into a handler error with the given
/// status code. This is handy if you want to specify the status code but still use the `?`
/// shorthand.
/// ```rust
/// # extern crate futures;
/// # extern crate gotham;
/// # use futures::executor::block_on;
/// # use gotham::anyhow::anyhow;
/// # use gotham::handler::{HandlerError, MapHandlerErrorFuture};
/// # use gotham::hyper::StatusCode;
/// # use std::future::Future;
/// fn handler() -> impl Future<Output = Result<(), HandlerError>> {
/// 	let result = async { Err(anyhow!("just a test")) };
/// 	result.map_err_with_status(StatusCode::IM_A_TEAPOT)
/// }
///
/// # #[allow(non_snake_case)]
/// # fn Err<T>(err: T) -> Result<(), T> {
/// #   Result::Err(err)
/// # }
/// # fn main() {
/// let response = block_on(handler());
/// assert_eq!(response.map_err(|err| err.status()), Err(StatusCode::IM_A_TEAPOT));
/// # }
/// ```
pub trait MapHandlerErrorFuture {
    /// Equivalent of `map_err(|err| HandlerError::from(err).with_status(status_code))`.
    fn map_err_with_status(self, status_code: StatusCode) -> MapErrWithStatus<Self>
    where
        Self: Sized;
}

impl<T, E, F> MapHandlerErrorFuture for F
where
    E: Into<anyhow::Error> + Display,
    F: Future<Output = Result<T, E>>,
{
    fn map_err_with_status(self, status_code: StatusCode) -> MapErrWithStatus<Self> {
        MapErrWithStatus::new(self, status_code)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("Dummy Error")]
    struct DummyError;

    fn error_prone() -> Result<(), HandlerError> {
        Err(DummyError.into())
    }

    #[test]
    fn test_error_downcast() {
        let mut err = error_prone().unwrap_err();
        assert!(err.downcast_cause_ref::<DummyError>().is_some());
        assert!(err.downcast_cause_mut::<DummyError>().is_some());
        assert!(err.downcast_cause_ref::<io::Error>().is_none());
        assert!(err.downcast_cause_mut::<io::Error>().is_none());
    }
}
