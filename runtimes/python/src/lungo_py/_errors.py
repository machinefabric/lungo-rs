"""Errors of calls into Lean programs."""


class LeanIOError(Exception):
    """An `IO.Error` of Lean: raised by a Lean function, or by a host function (raise
    `LeanIOError(message)`) to fail an `IO` extern with `IO.userError`."""

    def __init__(self, message, _handle=None):
        super().__init__(message)
        self.message = message
        self._handle = _handle


class LeanError(Exception):
    """The error value of an `EIO ε` function; a host function of an `EIO ε` extern raises it
    with its error value."""

    def __init__(self, value):
        super().__init__(value)
        self.value = value


class MalformedError(ValueError):
    """Arguments Lean cannot represent: an out-of-range integer, a value of the wrong type, a
    string with lone surrogates."""


class HostError(Exception):
    """A host function failed where Lean cannot observe the failure."""
