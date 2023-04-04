//!
//! mRPC uses a familiar gRPC status code, in order to mitigate the adaptation overhead.
//! This file is adapted from [`tonic/src/status.rs`][1].
//!
//! [1]: https://github.com/hyperium/tonic/blob/master/tonic/src/status.rs
use std::error::Error;
use std::fmt;

use phoenix_api::rpc::TransportStatus;

/// A gRPC status describing the result of an RPC call.
///
/// Values can be created using the `new` function or one of the specialized
/// associated functions.
/// ```rust
/// # use mrpc::{Status, Code};
/// let status1 = Status::new(Code::InvalidArgument, "name is invalid");
/// let status2 = Status::invalid_argument("name is invalid");
///
/// assert_eq!(status1.code(), Code::InvalidArgument);
/// assert_eq!(status1.code(), status2.code());
/// ```
pub struct Status {
    /// The gRPC status code, found in the `grpc-status` header.
    code: Code,
    /// A relevant error message, found in the `grpc-message` header.
    message: String,
    /// Optional underlying error.
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

/// gRPC status codes used by [`Status`].
///
/// These variants match the [gRPC status codes].
///
/// [gRPC status codes]: https://github.com/grpc/grpc/blob/master/doc/statuscodes.md#status-codes-and-their-use-in-grpc
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    /// The operation completed successfully.
    Ok = 0,

    /// The operation was cancelled.
    Cancelled = 1,

    /// Unknown error.
    Unknown = 2,

    /// Client specified an invalid argument.
    InvalidArgument = 3,

    /// Deadline expired before operation could complete.
    DeadlineExceeded = 4,

    /// Some requested entity was not found.
    NotFound = 5,

    /// Some entity that we attempted to create already exists.
    AlreadyExists = 6,

    /// The caller does not have permission to execute the specified operation.
    PermissionDenied = 7,

    /// Some resource has been exhausted.
    ResourceExhausted = 8,

    /// The system is not in a state required for the operation's execution.
    FailedPrecondition = 9,

    /// The operation was aborted.
    Aborted = 10,

    /// Operation was attempted past the valid range.
    OutOfRange = 11,

    /// Operation is not implemented or not supported.
    Unimplemented = 12,

    /// Internal error.
    Internal = 13,

    /// The service is currently unavailable.
    Unavailable = 14,

    /// Unrecoverable data loss or corruption.
    DataLoss = 15,

    /// The request does not have valid authentication credentials
    Unauthenticated = 16,
}

impl Code {
    /// Get description of this `Code`.
    /// ```
    /// fn make_grpc_request() -> mrpc::Code {
    ///     // ...
    ///     mrpc::Code::Ok
    /// }
    /// let code = make_grpc_request();
    /// println!("Operation completed. Human readable description: {}", code.description());
    /// ```
    /// If you only need description in `println`, `format`, `log` and other
    /// formatting contexts, you may want to use `Display` impl for `Code`
    /// instead.
    pub fn description(&self) -> &'static str {
        match self {
            Code::Ok => "The operation completed successfully",
            Code::Cancelled => "The operation was cancelled",
            Code::Unknown => "Unknown error",
            Code::InvalidArgument => "Client specified an invalid argument",
            Code::DeadlineExceeded => "Deadline expired before operation could complete",
            Code::NotFound => "Some requested entity was not found",
            Code::AlreadyExists => "Some entity that we attempted to create already exists",
            Code::PermissionDenied => {
                "The caller does not have permission to execute the specified operation"
            }
            Code::ResourceExhausted => "Some resource has been exhausted",
            Code::FailedPrecondition => {
                "The system is not in a state required for the operation's execution"
            }
            Code::Aborted => "The operation was aborted",
            Code::OutOfRange => "Operation was attempted past the valid range",
            Code::Unimplemented => "Operation is not implemented or not supported",
            Code::Internal => "Internal error",
            Code::Unavailable => "The service is currently unavailable",
            Code::DataLoss => "Unrecoverable data loss or corruption",
            Code::Unauthenticated => "The request does not have valid authentication credentials",
        }
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.description(), f)
    }
}

// ===== impl Status =====

impl Status {
    /// Create a new `Status` with the associated code and message.
    pub fn new(code: Code, message: impl Into<String>) -> Status {
        Status {
            code,
            message: message.into(),
            source: None,
        }
    }

    /// The operation completed successfully.
    pub fn ok(message: impl Into<String>) -> Status {
        Status::new(Code::Ok, message)
    }

    /// The operation was cancelled (typically by the caller).
    pub fn cancelled(message: impl Into<String>) -> Status {
        Status::new(Code::Cancelled, message)
    }

    /// Unknown error. An example of where this error may be returned is if a
    /// `Status` value received from another address space belongs to an error-space
    /// that is not known in this address space. Also errors raised by APIs that
    /// do not return enough error information may be converted to this error.
    pub fn unknown(message: impl Into<String>) -> Status {
        Status::new(Code::Unknown, message)
    }

    /// Client specified an invalid argument. Note that this differs from
    /// `FailedPrecondition`. `InvalidArgument` indicates arguments that are
    /// problematic regardless of the state of the system (e.g., a malformed file
    /// name).
    pub fn invalid_argument(message: impl Into<String>) -> Status {
        Status::new(Code::InvalidArgument, message)
    }

    /// Deadline expired before operation could complete. For operations that
    /// change the state of the system, this error may be returned even if the
    /// operation has completed successfully. For example, a successful response
    /// from a server could have been delayed long enough for the deadline to
    /// expire.
    pub fn deadline_exceeded(message: impl Into<String>) -> Status {
        Status::new(Code::DeadlineExceeded, message)
    }

    /// Some requested entity (e.g., file or directory) was not found.
    pub fn not_found(message: impl Into<String>) -> Status {
        Status::new(Code::NotFound, message)
    }

    /// Some entity that we attempted to create (e.g., file or directory) already
    /// exists.
    pub fn already_exists(message: impl Into<String>) -> Status {
        Status::new(Code::AlreadyExists, message)
    }

    /// The caller does not have permission to execute the specified operation.
    /// `PermissionDenied` must not be used for rejections caused by exhausting
    /// some resource (use `ResourceExhausted` instead for those errors).
    /// `PermissionDenied` must not be used if the caller cannot be identified
    /// (use `Unauthenticated` instead for those errors).
    pub fn permission_denied(message: impl Into<String>) -> Status {
        Status::new(Code::PermissionDenied, message)
    }

    /// Some resource has been exhausted, perhaps a per-user quota, or perhaps
    /// the entire file system is out of space.
    pub fn resource_exhausted(message: impl Into<String>) -> Status {
        Status::new(Code::ResourceExhausted, message)
    }

    /// Operation was rejected because the system is not in a state required for
    /// the operation's execution. For example, directory to be deleted may be
    /// non-empty, an rmdir operation is applied to a non-directory, etc.
    ///
    /// A litmus test that may help a service implementor in deciding between
    /// `FailedPrecondition`, `Aborted`, and `Unavailable`:
    /// (a) Use `Unavailable` if the client can retry just the failing call.
    /// (b) Use `Aborted` if the client should retry at a higher-level (e.g.,
    ///     restarting a read-modify-write sequence).
    /// (c) Use `FailedPrecondition` if the client should not retry until the
    ///     system state has been explicitly fixed.  E.g., if an "rmdir" fails
    ///     because the directory is non-empty, `FailedPrecondition` should be
    ///     returned since the client should not retry unless they have first
    ///     fixed up the directory by deleting files from it.
    pub fn failed_precondition(message: impl Into<String>) -> Status {
        Status::new(Code::FailedPrecondition, message)
    }

    /// The operation was aborted, typically due to a concurrency issue like
    /// sequencer check failures, transaction aborts, etc.
    ///
    /// See litmus test above for deciding between `FailedPrecondition`,
    /// `Aborted`, and `Unavailable`.
    pub fn aborted(message: impl Into<String>) -> Status {
        Status::new(Code::Aborted, message)
    }

    /// Operation was attempted past the valid range. E.g., seeking or reading
    /// past end of file.
    ///
    /// Unlike `InvalidArgument`, this error indicates a problem that may be
    /// fixed if the system state changes. For example, a 32-bit file system will
    /// generate `InvalidArgument if asked to read at an offset that is not in the
    /// range [0,2^32-1], but it will generate `OutOfRange` if asked to read from
    /// an offset past the current file size.
    ///
    /// There is a fair bit of overlap between `FailedPrecondition` and
    /// `OutOfRange`. We recommend using `OutOfRange` (the more specific error)
    /// when it applies so that callers who are iterating through a space can
    /// easily look for an `OutOfRange` error to detect when they are done.
    pub fn out_of_range(message: impl Into<String>) -> Status {
        Status::new(Code::OutOfRange, message)
    }

    /// Operation is not implemented or not supported/enabled in this service.
    pub fn unimplemented(message: impl Into<String>) -> Status {
        Status::new(Code::Unimplemented, message)
    }

    /// Internal errors. Means some invariants expected by underlying system has
    /// been broken. If you see one of these errors, something is very broken.
    pub fn internal(message: impl Into<String>) -> Status {
        Status::new(Code::Internal, message)
    }

    /// The service is currently unavailable.  This is a most likely a transient
    /// condition and may be corrected by retrying with a back-off.
    ///
    /// See litmus test above for deciding between `FailedPrecondition`,
    /// `Aborted`, and `Unavailable`.
    pub fn unavailable(message: impl Into<String>) -> Status {
        Status::new(Code::Unavailable, message)
    }

    /// Unrecoverable data loss or corruption.
    pub fn data_loss(message: impl Into<String>) -> Status {
        Status::new(Code::DataLoss, message)
    }

    /// The request does not have valid authentication credentials for the
    /// operation.
    pub fn unauthenticated(message: impl Into<String>) -> Status {
        Status::new(Code::Unauthenticated, message)
    }

    #[allow(dead_code)]
    pub(crate) fn from_error(err: Box<dyn Error + Send + Sync + 'static>) -> Status {
        Status::try_from_error(err).unwrap_or_else(|err| {
            let mut status = Status::new(Code::Unknown, err.to_string());
            status.source = Some(err);
            status
        })
    }

    pub(crate) fn try_from_error(
        err: Box<dyn Error + Send + Sync + 'static>,
    ) -> Result<Status, Box<dyn Error + Send + Sync + 'static>> {
        let err = match err.downcast::<Status>() {
            Ok(status) => {
                return Ok(*status);
            }
            Err(err) => err,
        };

        if let Some(mut status) = find_status_in_source_chain(&*err) {
            status.source = Some(err);
            return Ok(status);
        }

        Err(err)
    }

    #[allow(dead_code)]
    pub(crate) fn from_incoming_transport(transport_status: TransportStatus) -> Status {
        match transport_status {
            TransportStatus::Success => Status::ok(""),
            TransportStatus::Error(code) => match code.get() {
                402 => Status::permission_denied("Access Denied from server ACL engine"),
                _ => Status::data_loss(format!("receiving wc error: {code}")),
            },
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_outgoing_transport(transport_status: TransportStatus) -> Status {
        match transport_status {
            TransportStatus::Success => Status::ok(""),
            TransportStatus::Error(code) => Status::internal(format!("sending wc error: {code}")),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn map_error<E>(err: E) -> Status
    where
        E: Into<Box<dyn Error + Send + Sync>>,
    {
        let err: Box<dyn Error + Send + Sync> = err.into();
        Status::from_error(err)
    }

    /// Get the gRPC `Code` of this `Status`.
    pub fn code(&self) -> Code {
        self.code
    }

    /// Get the text error message of this `Status`.
    pub fn message(&self) -> &str {
        &self.message
    }
}

fn find_status_in_source_chain(err: &(dyn Error + 'static)) -> Option<Status> {
    let mut source = Some(err);

    while let Some(err) = source {
        if let Some(status) = err.downcast_ref::<Status>() {
            return Some(Status {
                code: status.code,
                message: status.message.clone(),
                // Since `Status` is not `Clone`, any `source` on the original Status
                // cannot be cloned so must remain with the original `Status`.
                source: None,
            });
        }

        source = err.source();
    }

    None
}

impl fmt::Debug for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A manual impl to reduce the noise of frequently empty fields.
        let mut builder = f.debug_struct("Status");

        builder.field("code", &self.code);

        if !self.message.is_empty() {
            builder.field("message", &self.message);
        }

        builder.field("source", &self.source);

        builder.finish()
    }
}

impl From<std::io::Error> for Status {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let code = match err.kind() {
            ErrorKind::BrokenPipe
            | ErrorKind::WouldBlock
            | ErrorKind::WriteZero
            | ErrorKind::Interrupted => Code::Internal,
            ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::NotConnected
            | ErrorKind::AddrInUse
            | ErrorKind::AddrNotAvailable => Code::Unavailable,
            ErrorKind::AlreadyExists => Code::AlreadyExists,
            ErrorKind::ConnectionAborted => Code::Aborted,
            ErrorKind::InvalidData => Code::DataLoss,
            ErrorKind::InvalidInput => Code::InvalidArgument,
            ErrorKind::NotFound => Code::NotFound,
            ErrorKind::PermissionDenied => Code::PermissionDenied,
            ErrorKind::TimedOut => Code::DeadlineExceeded,
            ErrorKind::UnexpectedEof => Code::OutOfRange,
            _ => Code::Unknown,
        };
        Status::new(code, err.to_string())
    }
}

impl From<crate::Error> for Status {
    fn from(err: crate::Error) -> Self {
        use crate::Error::*;
        // TODO(cjr): This mapping doesn't make sense at all.
        let code = match err {
            Service(..) | Interface(..) | Io(..) => Code::Internal,
            Serde(..) => Code::InvalidArgument,
            NoAddrResolved => Code::NotFound,
            Connect(..) => Code::Unavailable,
            Disconnect(..) => Code::Internal,
            ConnectionClosed => Code::Cancelled,
        };
        Status::new(code, err.to_string())
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "status: {:?}, message: {:?}",
            self.code(),
            self.message(),
        )
    }
}

impl Error for Status {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|err| (&**err) as _)
    }
}

// ===== impl Code =====

impl Code {
    /// Get the `Code` that represents the integer, if known.
    ///
    /// If not known, returns `Code::Unknown` (surprise!).
    pub fn from_i32(i: i32) -> Code {
        Code::from(i)
    }

    /// Convert the string representation of a `Code` (as stored, for example, in the `grpc-status`
    /// header in a response) into a `Code`. Returns `Code::Unknown` if the code string is not a
    /// valid gRPC status code.
    pub fn from_bytes(bytes: &[u8]) -> Code {
        match bytes.len() {
            1 => match bytes[0] {
                b'0' => Code::Ok,
                b'1' => Code::Cancelled,
                b'2' => Code::Unknown,
                b'3' => Code::InvalidArgument,
                b'4' => Code::DeadlineExceeded,
                b'5' => Code::NotFound,
                b'6' => Code::AlreadyExists,
                b'7' => Code::PermissionDenied,
                b'8' => Code::ResourceExhausted,
                b'9' => Code::FailedPrecondition,
                _ => Code::parse_err(),
            },
            2 => match (bytes[0], bytes[1]) {
                (b'1', b'0') => Code::Aborted,
                (b'1', b'1') => Code::OutOfRange,
                (b'1', b'2') => Code::Unimplemented,
                (b'1', b'3') => Code::Internal,
                (b'1', b'4') => Code::Unavailable,
                (b'1', b'5') => Code::DataLoss,
                (b'1', b'6') => Code::Unauthenticated,
                _ => Code::parse_err(),
            },
            _ => Code::parse_err(),
        }
    }

    fn parse_err() -> Code {
        Code::Unknown
    }
}

impl From<i32> for Code {
    fn from(i: i32) -> Self {
        match i {
            0 => Code::Ok,
            1 => Code::Cancelled,
            2 => Code::Unknown,
            3 => Code::InvalidArgument,
            4 => Code::DeadlineExceeded,
            5 => Code::NotFound,
            6 => Code::AlreadyExists,
            7 => Code::PermissionDenied,
            8 => Code::ResourceExhausted,
            9 => Code::FailedPrecondition,
            10 => Code::Aborted,
            11 => Code::OutOfRange,
            12 => Code::Unimplemented,
            13 => Code::Internal,
            14 => Code::Unavailable,
            15 => Code::DataLoss,
            16 => Code::Unauthenticated,

            _ => Code::Unknown,
        }
    }
}

impl From<Code> for i32 {
    #[inline]
    fn from(code: Code) -> i32 {
        code as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    #[derive(Debug)]
    struct Nested(Error);

    impl fmt::Display for Nested {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "nested error: {}", self.0)
        }
    }

    impl std::error::Error for Nested {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&*self.0)
        }
    }

    #[test]
    fn from_error_status() {
        let orig = Status::new(Code::OutOfRange, "weeaboo");
        let found = Status::from_error(Box::new(orig));

        assert_eq!(found.code(), Code::OutOfRange);
        assert_eq!(found.message(), "weeaboo");
    }

    #[test]
    fn from_error_unknown() {
        let orig: Error = "peek-a-boo".into();
        let found = Status::from_error(orig);

        assert_eq!(found.code(), Code::Unknown);
        assert_eq!(found.message(), "peek-a-boo".to_string());
    }

    #[test]
    fn from_error_nested() {
        let orig = Nested(Box::new(Status::new(Code::OutOfRange, "weeaboo")));
        let found = Status::from_error(Box::new(orig));

        assert_eq!(found.code(), Code::OutOfRange);
        assert_eq!(found.message(), "weeaboo");
    }

    #[test]
    fn code_from_i32() {
        // This for loop should catch if we ever add a new variant and don't
        // update From<i32>.
        for i in 0..(Code::Unauthenticated as i32) {
            let code = Code::from(i);
            assert_eq!(
                i, code as i32,
                "Code::from({}) returned {:?} which is {}",
                i, code, code as i32,
            );
        }

        assert_eq!(Code::from(-1), Code::Unknown);
    }

    #[test]
    fn constructors() {
        assert_eq!(Status::ok("").code(), Code::Ok);
        assert_eq!(Status::cancelled("").code(), Code::Cancelled);
        assert_eq!(Status::unknown("").code(), Code::Unknown);
        assert_eq!(Status::invalid_argument("").code(), Code::InvalidArgument);
        assert_eq!(Status::deadline_exceeded("").code(), Code::DeadlineExceeded);
        assert_eq!(Status::not_found("").code(), Code::NotFound);
        assert_eq!(Status::already_exists("").code(), Code::AlreadyExists);
        assert_eq!(Status::permission_denied("").code(), Code::PermissionDenied);
        assert_eq!(
            Status::resource_exhausted("").code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            Status::failed_precondition("").code(),
            Code::FailedPrecondition
        );
        assert_eq!(Status::aborted("").code(), Code::Aborted);
        assert_eq!(Status::out_of_range("").code(), Code::OutOfRange);
        assert_eq!(Status::unimplemented("").code(), Code::Unimplemented);
        assert_eq!(Status::internal("").code(), Code::Internal);
        assert_eq!(Status::unavailable("").code(), Code::Unavailable);
        assert_eq!(Status::data_loss("").code(), Code::DataLoss);
        assert_eq!(Status::unauthenticated("").code(), Code::Unauthenticated);
    }
}
