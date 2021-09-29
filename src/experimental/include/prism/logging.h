/*!
 *  Copyright (c) 2015 by Contributors
 * \file logging.h
 * \brief extract from dmlc/logging.h, glog compatible
 */
#ifndef PRISM_LOGGING_H_
#define PRISM_LOGGING_H_
#include <errno.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#if PRISM_LOG_STACK_TRACE
#include <cxxabi.h>
#endif

#if PRISM_LOG_STACK_TRACE
#include <execinfo.h>
#endif

namespace prism {
/*!
 * \brief exception class that will be thrown by
 *  default logger if PRISM_LOG_FATAL_THROW == 1
 */
struct Error : public std::runtime_error {
  /*!
   * \brief constructor
   * \param s the error message
   */
  explicit Error(const std::string& s) : std::runtime_error(s) {}
};
}  // namespace prism

#if defined(_MSC_VER) && _MSC_VER < 1900
#define noexcept(a)
#endif

#define PRISM_THROW_EXCEPTION noexcept(false)

#if PRISM_USE_GLOG
#include <glog/logging.h>

namespace prism {
inline void InitLogging(const char* argv0) { google::InitGoogleLogging(argv0); }
}  // namespace prism

#else
// use a light version of glog
#include <assert.h>
#include <ctime>
#include <iostream>
#include <sstream>

#if defined(_MSC_VER)
#pragma warning(disable : 4722)
#endif

namespace prism {
inline void InitLogging(const char* argv0) {
  // DO NOTHING
}

class LogCheckError {
 public:
  LogCheckError() : str(nullptr) {}
  explicit LogCheckError(const std::string& str_) : str(new std::string(str_)) {}
  ~LogCheckError() { if (str != nullptr) delete str; }
  operator bool() {return str != nullptr; }
  std::string* str;
};

#define DEFINE_CHECK_FUNC(name, op)                               \
  template <typename X, typename Y>                               \
  inline LogCheckError LogCheck##name(const X& x, const Y& y) {   \
    if (x op y) return LogCheckError();                           \
    std::ostringstream os;                                        \
    os << " (" << x << " vs. " << y << ") ";  /* CHECK_XX(x, y) requires x and y can be serialized to string. Use CHECK(x OP y) otherwise. NOLINT(*) */ \
    return LogCheckError(os.str());                               \
  }                                                               \
  inline LogCheckError LogCheck##name(int x, int y) {             \
    return LogCheck##name<int, int>(x, y);                        \
  }

#define CHECK_BINARY_OP(name, op, x, y)                               \
  if (prism::LogCheckError _check_err = prism::LogCheck##name(x, y))    \
    prism::LogMessageFatal(__FILE__, __LINE__).stream()                \
      << "Check failed: " << #x " " #op " " #y << *(_check_err.str)

#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wsign-compare"
DEFINE_CHECK_FUNC(_LT, <)
DEFINE_CHECK_FUNC(_GT, >)
DEFINE_CHECK_FUNC(_LE, <=)
DEFINE_CHECK_FUNC(_GE, >=)
DEFINE_CHECK_FUNC(_EQ, ==)
DEFINE_CHECK_FUNC(_NE, !=)
#pragma GCC diagnostic pop

// Always-on checking
#define CHECK(x)                                           \
  if (!(x))                                                \
    prism::LogMessageFatal(__FILE__, __LINE__).stream()     \
      << "Check failed: " #x << ' '
#define CHECK_LT(x, y) CHECK_BINARY_OP(_LT, <, x, y)
#define CHECK_GT(x, y) CHECK_BINARY_OP(_GT, >, x, y)
#define CHECK_LE(x, y) CHECK_BINARY_OP(_LE, <=, x, y)
#define CHECK_GE(x, y) CHECK_BINARY_OP(_GE, >=, x, y)
#define CHECK_EQ(x, y) CHECK_BINARY_OP(_EQ, ==, x, y)
#define CHECK_NE(x, y) CHECK_BINARY_OP(_NE, !=, x, y)
#define CHECK_NOTNULL(x)                                             \
  ((x) == NULL ? prism::LogMessageFatal(__FILE__, __LINE__).stream() \
                     << "Check  notnull: " #x << ' ',                \
   (x) : (x))  // NOLINT(*)
// Debug-only checking.
#ifdef NDEBUG
#define DCHECK(x) \
  while (false) CHECK(x)
#define DCHECK_LT(x, y) \
  while (false) CHECK((x) < (y))
#define DCHECK_GT(x, y) \
  while (false) CHECK((x) > (y))
#define DCHECK_LE(x, y) \
  while (false) CHECK((x) <= (y))
#define DCHECK_GE(x, y) \
  while (false) CHECK((x) >= (y))
#define DCHECK_EQ(x, y) \
  while (false) CHECK((x) == (y))
#define DCHECK_NE(x, y) \
  while (false) CHECK((x) != (y))
#else
#define DCHECK(x) CHECK(x)
#define DCHECK_LT(x, y) CHECK((x) < (y))
#define DCHECK_GT(x, y) CHECK((x) > (y))
#define DCHECK_LE(x, y) CHECK((x) <= (y))
#define DCHECK_GE(x, y) CHECK((x) >= (y))
#define DCHECK_EQ(x, y) CHECK((x) == (y))
#define DCHECK_NE(x, y) CHECK((x) != (y))
#endif  // NDEBUG

#define LOG_INFO prism::LogMessage(__FILE__, __LINE__)
#define LOG_ERROR LOG_INFO
#define LOG_WARNING LOG_INFO
#define LOG_FATAL prism::LogMessageFatal(__FILE__, __LINE__)
#define LOG_QFATAL LOG_FATAL

// Poor man version of VLOG
#define VLOG(x) LOG_INFO.stream()

#define LOG(severity) LOG_##severity.stream()
#define LG LOG_INFO.stream()
#define LOG_IF(severity, condition) \
  !(condition) ? (void)0 : prism::LogMessageVoidify() & LOG(severity)

// perror()..poor man style!
//
// PLOG() and PLOG_IF() and PCHECK() behave exactly like their LOG* and
// CHECK equivalents with the addition that they postpend a description
// of the current state of errno to their output lines.

#define PLOG_INFO prism::ErrnoLogMessage(__FILE__, __LINE__)
#define PLOG_ERROR PLOG_INFO
#define PLOG_WARNING PLOG_INFO
#define PLOG_FATAL prism::ErrnoLogMessageFatal(__FILE__, __LINE__)
#define PLOG_QFATAL PLOG_FATAL

#define PLOG(severity) PLOG_##severity.stream()

#define PLOG_IF(severity, condition) \
  !(condition) ? (void)0 : prism::LogMessageVoidify() & PLOG(severity)

// A CHECK() macro that postpends errno if the condition is false. E.g.
//
// if (poll(fds, nfds, timeout) == -1) { PCHECK(errno == EINTR); ... }
#define PCHECK(condition)                               \
  if (!(condition))                                     \
    prism::ErrnoLogMessageFatal(__FILE__, __LINE__).stream() \
        << "Check failed: " #condition << " "

#ifdef NDEBUG
#define LOG_DFATAL LOG_ERROR
#define DFATAL ERROR
#define DLOG(severity) \
  true ? (void)0 : prism::LogMessageVoidify() & LOG(severity)
#define DLOG_IF(severity, condition) \
  (true || !(condition)) ? (void)0 : prism::LogMessageVoidify() & LOG(severity)
#else
#define LOG_DFATAL LOG_FATAL
#define DFATAL FATAL
#define DLOG(severity) LOG(severity)
#define DLOG_IF(severity, condition) LOG_IF(severity, condition)
#endif

// Poor man version of LOG_EVERY_N
#define LOG_EVERY_N(severity, n) LOG(severity)

class DateLogger {
 public:
  DateLogger() {
#if defined(_MSC_VER)
    _tzset();
#endif
  }
  const char* HumanDate() {
#if defined(_MSC_VER)
    _strtime_s(buffer_, sizeof(buffer_));
#else
    time_t time_value = time(NULL);
    struct tm now;
    localtime_r(&time_value, &now);
    snprintf(buffer_, sizeof(buffer_), "%02d:%02d:%02d", now.tm_hour,
             now.tm_min, now.tm_sec);
#endif
    return buffer_;
  }

 private:
  char buffer_[9];
};

class LogMessage {
 public:
  LogMessage(const char* file, int line)
      :
#ifdef __ANDROID__
        log_stream_(std::cout)
#else
        log_stream_(std::cerr)
#endif
  {
    preserved_errno_ = errno;
    log_stream_ << "[" << pretty_date_.HumanDate() << "] " << file << ":"
                << line << ": ";
  }
  ~LogMessage() {
    // If errno was already set before we enter the logging call, we'll
    // set it back to that value when we return from the logging call.
    // It happens often that we log an error message after a syscall
    // failure, which can potentially set the errno to some other
    // values.  We would like to preserve the original errno.
    if (preserved_errno_ != 0) {
      errno = preserved_errno_;
    }
    log_stream_ << "\n";
  }
  std::ostream& stream() { return log_stream_; }

  int preserved_errno() const { return preserved_errno_; }

 protected:
  std::ostream& log_stream_;

 private:
  int preserved_errno_;
  DateLogger pretty_date_;
  LogMessage(const LogMessage&);
  void operator=(const LogMessage&);
};

inline int posix_strerror_r(int err, char* buf, size_t len) {
  // Sanity check input parameters
  if (buf == NULL || len <= 0) {
    errno = EINVAL;
    return -1;
  }

  // Reset buf and errno, and try calling whatever version of strerror_r()
  // is implemented by glibc
  buf[0] = '\000';
  int old_errno = errno;
  errno = 0;
  char* rc = reinterpret_cast<char*>(strerror_r(err, buf, len));

  // Both versions set errno on failure
  if (errno) {
    // Should already be there, but better safe than sorry
    buf[0] = '\000';
    return -1;
  }
  errno = old_errno;

  // POSIX is vague about whether the string will be terminated, although
  // is indirectly implies that typically ERANGE will be returned, instead
  // of truncating the string. This is different from the GNU implementation.
  // We play it safe by always terminating the string explicitly.
  buf[len - 1] = '\000';

  // If the function succeeded, we can use its exit code to determine the
  // semantics implemented by glibc
  if (!rc) {
    return 0;
  } else {
    // GNU semantics detected
    if (rc == buf) {
      return 0;
    } else {
      buf[0] = '\000';
#if defined(OS_MACOSX) || defined(OS_FREEBSD) || defined(OS_OPENBSD)
      if (reinterpret_cast<intptr_t>(rc) < sys_nerr) {
        // This means an error on MacOSX or FreeBSD.
        return -1;
      }
#endif
      strncat(buf, rc, len - 1);
      return 0;
    }
  }
}

inline std::string StrError(int err) {
  char buf[100];
  int rc = posix_strerror_r(err, buf, sizeof(buf));
  if ((rc < 0) || (buf[0] == '\000')) {
    snprintf(buf, sizeof(buf), "Error number %d", err);
  }
  return buf;
}

// Variables of type LogSeverity are widely taken to lie in the range
// [0, NUM_SEVERITIES-1].  Be careful to preserve this assumption if
// you ever need to change their values or add a new severity.
typedef int LogSeverity;

// Derived class for PLOG*() above.
class ErrnoLogMessage : public LogMessage {
 public:
  ErrnoLogMessage(const char* file, int line)
      : LogMessage(file, line) {}

  // Postpends ": strerror(errno) [errno]".
  ~ErrnoLogMessage() {
    // Don't access errno directly because it may have been altered
    // while streaming the message.
    stream() << ": " << StrError(preserved_errno()) << " [" << preserved_errno()
             << "]";
  }

 private:
  ErrnoLogMessage(const ErrnoLogMessage&);
  void operator=(const ErrnoLogMessage&);
};

#if PRISM_LOG_STACK_TRACE
inline std::string Demangle(char const* msg_str) {
  using std::string;
  string msg(msg_str);
  size_t symbol_start = string::npos;
  size_t symbol_end = string::npos;
  if (((symbol_start = msg.find("_Z")) != string::npos) &&
      (symbol_end = msg.find_first_of(" +", symbol_start))) {
    string left_of_symbol(msg, 0, symbol_start);
    string symbol(msg, symbol_start, symbol_end - symbol_start);
    string right_of_symbol(msg, symbol_end);

    int status = 0;
    size_t length = string::npos;
    std::unique_ptr<char, decltype(&std::free)> demangled_symbol = {
        abi::__cxa_demangle(symbol.c_str(), 0, &length, &status), &std::free};
    if (demangled_symbol && status == 0 && length > 0) {
      string symbol_str(demangled_symbol.get());
      std::ostringstream os;
      os << left_of_symbol << symbol_str << right_of_symbol;
      return os.str();
    }
  }
  return string(msg_str);
}

inline std::string StackTrace(const size_t stack_size = PRISM_LOG_STACK_TRACE_SIZE) {
  using std::string;
  std::ostringstream stacktrace_os;
  std::vector<void*> stack(stack_size);
  int nframes = backtrace(stack.data(), stack_size);
  stacktrace_os << "Stack trace returned " << nframes
                << " entries:" << std::endl;
  char** msgs = backtrace_symbols(stack.data(), nframes);
  if (msgs != nullptr) {
    for (int frameno = 0; frameno < nframes; ++frameno) {
      string msg = prism::Demangle(msgs[frameno]);
      stacktrace_os << "[bt] (" << frameno << ") " << msg << "\n";
    }
  }
  free(msgs);
  string stack_trace = stacktrace_os.str();
  return stack_trace;
}

#else  // PRISM_LOG_STACK_TRACE is off

inline std::string demangle(char const* msg_str) { return std::string(); }

inline std::string StackTrace(const size_t stack_size = 0) {
  return std::string(
      "stack traces not available when "
      "PRISM_LOG_STACK_TRACE is disabled at compile time.");
}

#endif  // PRISM_LOG_STACK_TRACE

#if PRISM_LOG_FATAL_THROW == 0
class LogMessageFatal : public LogMessage {
 public:
  LogMessageFatal(const char* file, int line) : LogMessage(file, line) {}
  ~LogMessageFatal() {
    log_stream_ << "\n\n" << StackTrace() << "\n";
    abort();
  }

 private:
  LogMessageFatal(const LogMessageFatal&);
  void operator=(const LogMessageFatal&);
};

class ErrnoLogMessageFatal : public ErrnoLogMessage {
 public:
  ErrnoLogMessageFatal(const char* file, int line) : ErrnoLogMessage(file, line) {}
  ~ErrnoLogMessageFatal() {
    stream() << ": " << StrError(preserved_errno()) << " [" << preserved_errno()
             << "]";
    stream() << "\n\n" << StackTrace() << "\n";
    abort();
  }

 private:
  ErrnoLogMessageFatal(const ErrnoLogMessageFatal&);
  void operator=(const ErrnoLogMessageFatal&);
};
#else
class LogMessageFatal {
 public:
  LogMessageFatal(const char* file, int line) {
    log_stream_ << "[" << pretty_date_.HumanDate() << "] " << file << ":"
                << line << ": ";
  }
  std::ostringstream& stream() { return log_stream_; }
  ~LogMessageFatal() PRISM_THROW_EXCEPTION {
#if PRISM_LOG_STACK_TRACE
    log_stream_ << "\n\n" << StackTrace() << "\n";
#endif
    // throwing out of destructor is evil
    // hopefully we can do it here
    // also log the message before throw
    LOG(ERROR) << log_stream_.str();
    throw Error(log_stream_.str());
  }

 private:
  std::ostringstream log_stream_;
  DateLogger pretty_date_;
  LogMessageFatal(const LogMessageFatal&);
  void operator=(const LogMessageFatal&);
};
#endif

// This class is used to explicitly ignore values in the conditional
// logging macros.  This avoids compiler warnings like "value computed
// is not used" and "statement has no effect".
class LogMessageVoidify {
 public:
  LogMessageVoidify() {}
  // This has to be an operator with a precedence lower than << but
  // higher than "?:". See its usage.
  void operator&(std::ostream&) {}
};

}  // namespace prism

#endif
#endif  // PRISM_LOGGING_H_
