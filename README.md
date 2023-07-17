# The Whenever Task Scheduler

<!-- @import "[TOC]" {cmd="toc" depthFrom=1 depthTo=6 orderedList=false} -->

<!-- code_chunk_output -->

- [The Whenever Task Scheduler](#the-whenever-task-scheduler)
  - [Introduction](#introduction)
  - [Features](#features)
  - [CLI](#cli)
  - [Configuration](#configuration)
    - [Globals](#globals)
    - [Tasks](#tasks)
      - [Command tasks](#command-tasks)
      - [Lua script tasks](#lua-script-tasks)
    - [Conditions](#conditions)
      - [Interval](#interval)
      - [Time](#time)
      - [Idle session](#idle-session)
      - [Command](#command)
      - [Lua script](#lua-script)
      - [DBus method](#dbus-method)
      - [Event based conditions](#event-based-conditions)
    - [Events](#events)
      - [Filesystem changes](#filesystem-changes)
      - [DBus signals](#dbus-signals)
  - [Logging](#logging)
  - [Conclusion](#conclusion)
  - [License](#license)

<!-- /code_chunk_output -->

**whenever** is a simple task scheduler capable of executing _tasks_ (OS commands and _Lua_ scripts) according to specific _conditions_. Conditions are of various types: depending on time (both intervals or specific more-or-less defined instants), execution of OS commands or _Lua_ scripts, changes in specific files and directories, session inactivity, DBus signals or property checks[^1]. The scheduler intends to be as lightweight as possible in terms of used computational resources, and to run at a low priority level.

Configuration is provided to the scheduler via a [TOML](https://toml.io/) file, which must contain all definitions for conditions and associated tasks, as well as events that the scheduler should listen to.

Ideally, **whenever** is the successor of the _Python_ based [_When_](https://github.com/almostearthling/when-command) scheduler, with the intention of being cross platform, more efficient and as least resource-consuming as possible. It also gained some features (eg. _Lua_ scripting) that _When_ did not have, at no cost in terms of performance since **whenever** is a self-contained, optimized, and thin executable instead of being an interpreted program.

Although a command line application, it is designed for desktops -- therefore it should be executed via a controlling GUI wrapper.


## Introduction

The purpose of **whenever** is to provide the user, possibly without administrative credentials, with the ability to define conditions that do not only depend on time, but also on particular states of the session, result of commands run in a shell, execution of _Lua_ scripts, or other events that may occur while the system is being used. This scheduler is a terminal (or console, on Windows) application, however it is meant to run in the background without interaction with the user. The application is able to produce detailed logs, so that the user can review what the application is doing or has done.

Just like its predecessor, **whenever** overlaps to some extent with the standard _cron_ scheduler on Unix, and with the _Task Scheduler_ on Windows. However this scheduler tries to be more flexible -- although less precise than _cron_ -- and to function as an alternative to more complex solutions that could be implemented using the system-provided schedulers. The **whenever** approach is to perform certain tasks after a condition is met, in a relaxed fashion: this means that the tasks might not be performed _exactly_ in the instant that marks the condition verification, but _after_ such verification instead. Thus this scheduler is not intended as a replacement for the utilities provided by the operating system: it aims at providing an easy solution to those who need to automate some actions depending on other situations or events that may occur.

Also, **whenever** aims at being cross-platform: until now, all features are available on all supported operating systems -- although in some cases part of these features (DBus support, for example) can be of little or no use on some supported environments. In opposition to its predecessor, **whenever** tries to be conservative in terms of resource cosumption (especially CPU and RAM), and, since it does not interact with the user normally, it should be able to run at low priority. Therefore, **whenever** does not implement a GUI by itself: on the contrary, it implements a simple _stdin_-based interface that is mostly aimed at interacting with an independent _wrapper_. Also, no _persistence_ is implemented in this version. The actions to perform are loaded every time at startup by means of a single configuration file that, as many modern tools do, uses the well known TOML format[^2].

A very lightweight cross-platform wrapper will soon be available (it is actually ready, under test on Windows, to be checked on Linux and possibly other operating systems), written in C++ and using the [wxWidgets](https://www.wxwidgets.org/) GUI library: it has been designed to implement the bare minimum of functionality and to just show an icon in the system tray area, from which it is possible to stop the scheduler, and to pause/resume the condition checks and therefore the execution of tasks that would derive from them. The lightweight wrapper also hides the console window on Windows environments. Due to the use of _stdin_/_stdout_ for communication, it is possible to build more elaborate wrappers in any language that supports the possibility to spawn a process and control its I/O, at the expense of a larger occupation of the resources but possibly without drawbacks in terms of performance, as the scheduler runs in a separate task anyway. The _Python_ based _When_ application had an occupation in RAM of about 70MB on Ubuntu Linux using a not-too-populated configuration file, and could noticeably use the CPU: this version, written in the [_Rust_](https://www.rust-lang.org/) programming language, needs around 1.5MB of RAM on Windows[^3] when using a configuration file that tests all possible types of _task_, _condition_, and _event_ supported on the platform. Nevertheless, **whenever** is fully multithreaded, condition checks have no influence on each other and, when needed, may run concurrently. Consequential task execution also takes place with full parallelism -- with the exception of tasks that, per configuration, _must_ run sequentially.


## Features

**whenever** can perform the following types of [**_task_**](#tasks):

* [_Execution of groups of OS executables_](#command-tasks), either sequentially or concurrently, checking their exit code or output (both on _stdout_ and _stderr_) for expected or undesired results
* [_Execution of_ Lua _scripts_](#lua-script-tasks), using an embedded interpreter, with the possibility of checking the contents of _Lua_ variables for expected outcomes

as the consequence of the verification of a **_condition_**. The concepts of tasks and conditions are inherited from the _Python_ based **When** scheduler: how tasks and conditions work is almost identical in both tools -- in fact, a tool will be made available to convert from **When** _export files_ to **whenever** configuration files.

The supported types of [**_condition_**](#conditions) are the following:

* [_Interval_ based](#interval): the _periodic_ conditions are verified after a certain time interval has passed since **whenever** has started, and may be verified again after the same amount of time if the condition is set to be _recurrent_
* [_Time_ based](#time): one or more instants in time can be specified when the condition is verified
* [_Idle_ user session](#command): this type of condition is verified after the session has been idle for the specified amount of time
* [_Command_ execution](#command): an available executable (be it a script, a batch file on Windows, a binary) is run, its exit code or output is checked and, when an expected outcome is found, the condition is considered verified - or failed on an explicitly undesired outcome
* [_Lua_ script execution](#lua-script): a _Lua_ script is run using the embedded interpreter, and if the contents of one or more variables meet the specified expectations the condition is considered verified
* [_DBus_ inspection](#dbus-method): a _DBus_ method is executed and the result is checked against some criteria provided in the configuration file
* [_Event_ based](#event-based-conditions): are verified when a certain event occurs that fires the condition.

The [**_events_**](#events) that can fire _event_ based conditions are, at the moment:

* [_Filesystem changes_](#filesystem-changes), that is, changes in files and/or directories that are set to be monitored
* [_DBus signals_](#dbus-signals) that may be filtered for an expected payload.

Note that _DBus_ events and conditions are also (theoretically) supported on Windows, being one of the _DBus_ target platforms.

All of the above listed items are fully configurable via a TOML configuration file, that _must_ be specified as the only mandatory argument on the command line. The syntax of the configuration file is described in the following sections.

Every type of check is performed periodically, even the ones involving _event_ based conditions[^4]: the times at which the conditions are checked is usually referred here as _tick_ and the tick interval can be specified in the configuration file -- defaulting at 5 seconds. Note that, since performing all checks in the same instant at every tick could cause usage peaks in terms of computational resources, there is the option to attempt to randomly distribute some of the checks within the tick interval, by explicitly specifying this behaviour in the configuration file.


## CLI

The command can be directly invoked as a foreground process from the command line. This is particularly useful (especially with full logging enabled) to debug the configuration. **whenever** either logs to the console or on a specified file. When logging to the console, different colors are used by default to visually accentuate messages related to different logging levels.

By invoking **whenever** specifying `--help` as argument, the output is the following:

```shell
~$ whenever --help
A simple background job launcher and scheduler

Usage: whenever [OPTIONS] <CONFIG>

Arguments:
  <CONFIG>  Path to configuration file

Options:
  -q, --quiet              Suppress all output (requires logfile to be specified)
  -l, --log <LOGFILE>      Specify the log file
  -L, --log-level <LEVEL>  Specify the log level (default: warn) [default: warn] [possible values: trace, debug, info, warn, error]
  -a, --log-append         Append to an existing log file if found
  -P, --log-plain          No colors when logging (default for log files)
  -C, --log-color          Use colors when logging to console (default, ignored with log files)
  -J, --log-json           Use JSON format for logging
  -h, --help               Print help
  -V, --version            Print version
```

As the available options are minimal, and mostly impact on [logging](#logging), the only elements that should be highlighted are the following:

* **whenever** requires a log file to run in _quiet_ mode (which also suppresses errors)
* it is possible to suppress colors on console logging, by specifying `--log-plain` to the command
* when run within a wrapper, **whenever** can emit log messages in the JSON format, to make it easier for the wrapper to interpret and classify them.

When debugging a configuration file, it might be useful to set the log level at least to _debug_, if not to _trace_ which also emits some redundant messages.

An important thing to notice is that configuration errors will cause **whenever** to abort, by issuing a very brief message on the console.


## Configuration

The configuration file is strictly based on the current TOML specification: therefore it can be easily implemented by hand, or automatically written (for example, by a GUI based utility) using a library capable of writing well-formed TOML files. This section describes the exact format of this file, in all of its components.


### Globals

Globals must be specified at the beginning of the configuration file. The supported global entries are the following:

| Option                          | Default | Description                                                                          |
|---------------------------------|---------|--------------------------------------------------------------------------------------|
| `scheduler_tick_seconds`        | 5       | Number of seconds between scheduler ticks                                            |
| `randomize_checks_within_ticks` | _false_ | Whether or not condition checks hould be uniformly randomized within the tick period |

Both parameters can be omitted, in which case the default values are used: 5 seconds might seem a very short value for the tick period, but in fact it mimics a certain responsiveness and synchronicity in checking _event_ based conditions. Note that conditions strictly depending on time do not comply to the request of randomizing the check time.


### Tasks

_Tasks_ are handled first in this document, because _conditions_ must mandatorily specify the tasks to be executed upon verification. There are two types of task, each of which is described in detail in its specific subsection.

Tasks are defined via a dedicated table, which means that every task definition must start with the TOML `[[task]]` section header.

Task names are mandatory, and must be provided as alphanumeric strings (may include underscores), beginning with a letter. The task type must be either `"command"` or `"lua"` according to what is configured, any other value is considered a configuration error.

#### Command tasks

_Command_ based tasks actually execute commands at the OS level: they might have a _positive_ as well as a _negative_ outcome, depending on user-provided criteria. As said above, these criteria may not just depend on the exit code of the executed command, but also on checks performed on their output taking either the standard output or the standard error channels into account. By default no check is performed, but the user can choose, for instance, to consider a zero exit code as a successful execution (quite common for OS commands). It is possible to consider another exit code as successful, or the zero exit code as a failure (for instance, if a file should not be found, performing `ls` on it would have the zero exit code as an _undesirable_ outcome). Also, a particular substring can be sought in the standard output or standard error streams both as expected or as unexpected. The two streams can be matched against a provided _regular expression_ if just seeking a certain substring is not fine-grained enough. Both substrings and regular expressions can be respectively sought or matched either case-sensitively or case-insensitively.

A sample configuration for a command based task is the following:

```toml
[[task]]
name = "CommandTaskName"
type = "command"
startup_path = "/some/startup/directory"    # must exist
command = "executable_name"
command_arguments = [
    "arg1",
    "arg2",
    ]

# optional parameters (if omitted, defaults are used)
match_exact = false
match_regular_expression = false
success_stdout = "expected"
success_stderr = "expected_error"
success_status = 0
failure_stdout = "unexpected"
failure_stderr = "unexpected_error"
failure_status = 2
timeout_seconds = 60
case_sensitive = false
include_environment = false
set_envvironment_variables = false
environment_variables = { VARNAME1 = "value1", VARNAME2 = "value2" }
```

and the following table provides a detailed description of the entries:

| Entry                       | Default | Description                                                                                                     |
|-----------------------------|:-------:|-----------------------------------------------------------------------------------------------------------------|
| `name`                      | N/A     | the unique name of the task (mandatory)                                                                         |
| `type`                      | N/A     | must be set to `"command"` (mandatory)                                                                          |
| `startup_path`              | N/A     | the directory in which the command is started                                                                   |
| `command`                   | N/A     | path to the executable (mandatory; if the path is omitted, the executable should be found in the search _PATH_) |
| `command_arguments`         | N/A     | arguments to pass to the executable: can be an empty list, `[]` (mandatory)                                     |
| `match_exact`               | _false_ | if _true_, the entire output is matched instead of searching for a substring                                    |
| `match_regular_expression`  | _false_ | if _true_, the match strings are considered regular expressions instead of substrings                           |
| `case_sensitive`            | _false_ | if _true_, substring search or match and regular expressions match is performed case-sensitively                |
| `timeout_seconds`           | (empty) | if set, the number of seconds to wait before the command is terminated (with unsuccessful outcome)              |
| `success_status`            | (empty) | if set, when the execution ends with the provided exit code the task is considered successful                   |
| `failure_status`            | (empty) | if set, when the execution ends with the provided exit code the task is considered failed                       |
| `success_stdout`            | (empty) | the substring or RE to be found or matched on _stdout_ to consider the task successful                          |
| `success_stderr`            | (empty) | the substring or RE to be found or matched on _stderr_ to consider the task successful                          |
| `failure_stdout`            | (empty) | the substring or RE to be found or matched on _stdout_ to consider the task failed                              |
| `failure_stderr`            | (empty) | the substring or RE to be found or matched on _stderr_ to consider the task failed                              |
| `include_environment`       | _false_ | if _true_, the command is executed in the same environment in which **whenever** was started                    |
| `set_environment_variables` | _false_ | if _true_, **whenever** sets environment variables reporting the names of the task and the condition            |
| `environment_variables`     | `{}`    | extra variables that might have to be set in the environment in which the provided command runs                 |

The priority used by **whenever** to determine success or failure in the task is the one in which the related parameters appear in the above table: first exit codes are checked, then both _stdout_ and _stderr_ are checked for substrings ore regular expressions that identify success, and finally the same check is performed on values that indicate a failure. Most of the times just one or maybe two of the expected parameters will have to be set. Note that the command execution is not considered successful with a zero exit code by default, nor a failure on a nonzero exit code: both assumptions have to be explicitly configured by setting either `success_status` or `failure_status`. If a command is known to have the possibility to hang, a timeout can be configured by specifying the maximum number of seconds to wait for the process to exit: after this amount of time the process is terminated and fails.

If `set_environment_variables` is _true_, **whenever** sets the following environment variables:

* `WHENEVER_TASK` to the unique name of the task
* `WHENEVER_CONDITION` to the unique name of the condition that triggered the task

for scripts or other executables that might be aware of **whenever**.

#### Lua script tasks

Tasks based on [_Lua_](https://www.lua.org/) scripts might be useful when an action has to be performed that requires a non-trivial sequence of operations but for which it would be excessive to write a specific script to be run as a command. The script to be run is embedded directly in the configuration file -- TOML helps in this sense, by allowing multiline strings to be used in its specification.

_Lua_ based tasks can be considered more lightweight than _command_ tasks, as the interpreter is embedded in **whenever**. Also, the embedded _Lua_ interpreter is enriched with library functions that allow to write to the **whenever** log, at all logging levels (_error_, _warn_, _info_, _debug_, _trace_). The library functions are the following:

* `log.error`
* `log.warn`
* `log.info`
* `log.debug`
* `log.trace`

and take a single string as their argument.

The configuration of _Lua_ based tasks has the following form:

```toml
[[task]]
name = "LuaTaskName"
type = "lua"
script = '''
    log.info("hello from Lua");
    result = 10;
    '''

# optional parameters (if omitted, defaults are used)
expect_all = false
expected_results = { result = 10 }
```

and the following table provides a detailed description of the entries:

| Entry              | Default | Description                                                                                                    |
|--------------------|:-------:|----------------------------------------------------------------------------------------------------------------|
| `name`             | N/A     | the unique name of the task (mandatory)                                                                        |
| `type`             | N/A     | must be set to `"lua"` (mandatory)                                                                             |
| `script`           | N/A     | the _Lua_ code that has to be executed by the internal interpreter (mandatory)                                 |
| `expect_all`       | _false_ | if _true_, all the expected results have to be matched to consider the task successful, otherwise at least one |
| `expected_results` | `{}`    | a dictionary of variable names and their expected values to be checked after execution                         |

Note that _triple single quotes_ have been used to embed the script: this allows to use escapes and quotes in the script itself. Although the script should be embedded in the configuration file, it is possible to execute external scripts via `dofile("/path/to/script.lua")` or by using the `require` function. While a successful execution is always determined by matching the provided criteria, an error in the script is always considered a failure.

From the embedded _Lua_ interpreter there are two values set that can be accessed:

* `whenever_task` is the name of the task that executes the script
* `whenever_condition` is the name of the condition that triggered the task.

which might be useful if the scripts are aware of being run within **whenever**.


### Conditions

_Conditions_ are at the heart of **whenever**, by triggering the execution of tasks. As mentioned above, several types of condition are supported. Part of the configuration is common to all conditions, that is:

| Entry              | Default | Description                                                                                                    |
|--------------------|:-------:|----------------------------------------------------------------------------------------------------------------|
| `name`             | N/A     | the unique name of the condition (mandatory)                                                                   |
| `type`             | N/A     | string describing the type of condition (mandatory, one of the possible values)                                |
| `recurring`        | _false_ | if _false_, the condition is not checked anymore after first successful verification                           |
| `execute_sequence` | _true_  | if _true_ the associated tasks are executed one after the other, in the order in which they are listed         |
| `break_on_success` | _false_ | if _true_, task execution stops after the first successfully executed task (when `execute_sequence` is _true_) |
| `break_on_failure` | _false_ | if _true_, task execution stops after the first failed task (when `execute_sequence` is _true_)                |
| `suspended`        | _false_ | if _true_, the condition will not be checked nor the associated tasks executed                                 |
| `tasks`            | `[]`    | a list of task names that will be executed upon condition verification                                         |

When `execute_sequence` is set to _false_, the associated tasks are started concurrently in the same instant, and task outcomes are ignored. Otherwise a minimal control flow is implemented, allowing the sequence to be interrupted after the first success or failure in task execution. Note that it is possible to set both `break_on_success` and `break_on_failure` to _true_[^5].

The `type` entry can be one of: `"interval"`, `"time"`, `"idle"`, `"command"`, `"lua"`, `"event"`, and `"dbus"`. Any other value is considered a configuration error.

For conditions that should be periodically checked and whose associated task list has to be run _whenever_ they occur (and not just after the first occurrence), the `recurring` entry can be set to _true_. The `suspended` entry can assume a _true_ value for conditions for which the user does not want to remove the configuration but should be (at least temporarily) prevented.

Another entry is common to several condition types, that is `check_after`: it can be set to the number of seconds that **whenever** has to wait after startup (and after the last check for _recurring_ conditions) for a subsequent check: this is useful for conditions that can run on a more relaxed schedule, or whose check process has a significant cost in terms of resources, or whose associated task sequence might take a long time to finish. Simpler conditions and conditions based on time do not accept this entry.

Note that all listed tasks must be defined, otherwise an error is raised and **whenever** will not start.

The following paragraphs describe in detail each condition type. For the sake of brevity, only specific configuration entries will be described for each type.

All _condition_ definition sections must start with the TOML `[[condition]]` header.

#### Interval

_Interval_ based conditions are verified after a certain amount of time has passed, either since startup or after the last successful check. This type of condition is useful for tasks that should be executed periodically, thus most of the times `recurring` will be set to _true_ for this type of condition. The following is an example of interval based condition:

```toml
[[condition]]
name = "IntervalConditionName"
type = "interval"
interval_seconds = 3600

# optional parameters (if omitted, defaults are used)
recurring = false
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = true
tasks = [
    "Task1",
    "Task2",
    ]
```

describing a condition that is verified one hour after **whenever** has started, and not anymore after the first occurrence (because `recurring` is _false_ here). Were it _true_, the condition would be verified _every_ hour.

The specific parameters for this type of condition are:

| Entry              | Default | Description                                                                |
|--------------------|:-------:|----------------------------------------------------------------------------|
| `type`             | N/A     | has to be set to `"interval"` (mandatory)                                  |
| `interval_seconds` | N/A     | the number of seconds to wait for the condition to be verified (mandatory) |

The check for this type of condition is never randomized.

#### Time

_Time_ based conditions occur just after one of the provided time specifications has been reached. Time specifications are given as a list of tables, each of which can contain one or more of the following entries:

* `hour`: the hour, as an integer between 0 and 23
* `minute`: the minute, as an integer between 0 and 59
* `second`: the second, as an integer between 0 and 59
* `year`: an integer expressing the (full) year
* `month`: an integer expressing the month, between 1 (January) and 12 (December)
* `day`: an integer expressing the day of themonth, between 1 and 31
* `weekday`: the name of the weekday in English, either whole or abbreviated to three letters.

Not all the entries must be specified: for instance, specifying the day of week and a full date (as year, month, date) may cause the event to never occur if that particular date does not occur on that specific week day. Normally a day of the month will be specified, and then a time of the day, or a weekday and a time of the day. However full freedom is given in specifying or omitting part of the date:

* missing parts in the date will be considered verified at every change of each of them (years, months, days, and weekdays)
* a missing hour specification will be considered verified at every hour
* a missing minute or second specification will be considered verified respectively at the first minute of the hour and first second of the minute.

Of course, all the time specifications in the provided list will be checked at each tick: this allows complex configurations for actions that must be performed at specific times.

A sample configuration section follows:

```toml
[[condition]]
name = "TimeConditionName"
type = "time"                               # mandatory value

# optional parameters (if omitted, defaults are used)
time_specifications = [
    { hour = 17, minute = 30 },
    { hour = 12, minute = 0, weekday = "wed" },
    ]
recurring = true
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = true
tasks = [
    "Task1",
    "Task2",
    ]
```

for a condition that is verified everyday at 5:30PM and every Wednesday at noon. The specific parameters are:

| Entry                 | Default | Description                                                                                                     |
|-----------------------|:-------:|-----------------------------------------------------------------------------------------------------------------|
| `type`                | N/A     | has to be set to `"time"` (mandatory)                                                                           |
| `time_specifications` | `{}`    | a list of _partial_ time specifications, as inline tables consisting of the above described entries (mandatory) |

The check for this type of condition is never randomized.

#### Idle session

Conditions of the _idle_ type are verified after the session has been idle (that is, without user interaction), for the specified number of seconds[^6]. This does normally not interfere with other idle time based actions provided by the environment such as screensavers, and automatic session lock. The following is a sample configuration for this type of condition:

```toml
[[condition]]
name = "IdleConditionName"
type = "idle"
idle_seconds = 3600

# optional parameters (if omitted, defaults are used)
recurring = true
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = true
tasks = [
    "Task1",
    "Task2",
    ]
```

for a condition that will be verified each time that an hour has passed since the user has been away from the mouse and the keyboard. For tasks that usually occur only once per session when the workstation is idle (such as backups, for instance), `recurring` can be set to _false_. The table below describes the specific configuration entries:

| Entry          | Default | Description                                                                                         |
|----------------|:-------:|-----------------------------------------------------------------------------------------------------|
| `type`         | N/A     | has to be set to `"idle"` (mandatory)                                                               |
| `idle_seconds` | N/A     | the number of idle seconds to be waited for in order to consider the condition verified (mandatory) |

The check for this type of condition is never randomized.

#### Command

This type of condition gives the possibility to execute an OS _command_ and decide whether or not the condition is verified testing the command exit code and/or what the command writes on its standard output or standard error channel. The available checks are of the same type as the ones available for command based tasks. In fact it is possible to:

* identify a provided exit code as a failure or as a success
* specifying that the presence of a substring or matching a regular expression corresponds to either a failure or a success.

Of course only a success causes the corresponding tasks to be executed: as for command based tasks, it is not mandatory to follow the usual conventions -- this means, for instance, that a zero exit code can be identified as a failure and a non-zero exit code as a success. A non-success has the same effect as a failure on task execution.

If a command is known to have the possibility to hang, a timeout can be configured by specifying the maximum number of seconds to wait for the process to exit: after this amount of time the process is terminated and fails.

An example of command based condition follows:

```toml
[[condition]]
name = "CommandConditionName"
type = "command"                            # mandatory value

startup_path = "/some/startup/directory"    # must exist
command = "executable_name"
command_arguments = [
    "arg1",
    "arg2",
    ]

# optional parameters (if omitted, defaults are used)
recurring = false
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = false
tasks = [
    "Task1",
    "Task2",
    ]
check_after = 10

match_exact = false
match_regular_expression = false
success_stdout = "expected"
success_stderr = "expected_error"
success_status = 0
failure_stdout = "unexpected"
failure_stderr = "unexpected_error"
failure_status = 2
timeout_seconds = 60
case_sensitive = false
include_environment = true
set_environment_variables = true
environment_variables = { VARNAME1 = "value1", VARNAME2 = "value2" }
```

The following table illustrates the parameters specific to _command_ based conditions:

| Entry                       | Default | Description                                                                                                                  |
|-----------------------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `type`                      | N/A     | has to be set to `"interval"` (mandatory)                                                                                    |
| `check_after`               | _empty_ | number of seconds that have to pass before the condition is checked the first time or further times if `recurrent` is _true_ |
| `startup_path`              | N/A     | the directory in which the command is started (mandatory)                                                                    |
| `command`                   | N/A     | path to the executable (mandatory; if the path is omitted, the executable should be found in the search _PATH_)              |
| `command_arguments`         | N/A     | arguments to pass to the executable: can be an empty list, `[]` (mandatory)                                                  |
| `match_exact`               | _false_ | if _true_, the entire output is matched instead of searching for a substring                                                 |
| `match_regular_expression`  | _false_ | if _true_, the match strings are considered regular expressions instead of substrings                                        |
| `case_sensitive`            | _false_ | if _true_, substring search or match and regular expressions match is performed case-sensitively                             |
| `timeout_seconds`           | (empty) | if set, the number of seconds to wait before the command is terminated (with unsuccessful outcome)                           |
| `success_status`            | _empty_ | if set, when the execution ends with the provided exit code the condition is considered verified                             |
| `failure_status`            | N/A     | if set, when the execution ends with the provided exit code the condition is considered failed                               |
| `success_stdout`            | (empty) | the substring or RE to be found or matched on _stdout_ to consider the task successful                                       |
| `success_stderr`            | (empty) | the substring or RE to be found or matched on _stderr_ to consider the task successful                                       |
| `failure_stdout`            | (empty) | the substring or RE to be found or matched on _stdout_ to consider the task failed                                           |
| `failure_stderr`            | (empty) | the substring or RE to be found or matched on _stderr_ to consider the task failed                                           |
| `include_environment`       | _false_ | if _true_, the command is executed in the same environment in which **whenever** was started                                 |
| `set_environment_variables` | _false_ | if _true_, **whenever** sets environment variables reporting the names of the task and the condition                         |
| `environment_variables`     | `{}`    | extra variables that might have to be set in the environment in which the provided command runs                              |

If `set_environment_variables` is _true_, **whenever** sets the following environment variable:

* `WHENEVER_CONDITION` to the unique name of the condition that is currently being tested

for scripts or other executables used in checks that might be aware of **whenever**.

For this type of condition the actual test can be performed at a random time within the tick interval.

#### Lua script

A [_Lua_](https://www.lua.org/) script can be used to determine the verification of a condition: after the execution of the script, one or more variables can be checked against expected values and thus decide whether or not the associated tasks have to be run. Given the power of _Lua_ and its standard library, this type of condition can constitute a lightweight alternative to complex scripts to call to implement a _command_ based condition. The definition of a _Lua_ condition is actually much simpler:

```toml
[[condition]]
name = "LuaConditionName"
type = "lua"                                # mandatory value
script = '''
    log.info("hello from Lua");
    result = 10;
    '''

# optional parameters (if omitted, defaults are used)
recurring = false
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = false
tasks = [
    "Task1",
    "Task2",
    ]
check_after = 10
expect_all = false
expected_results = { result = 10 }
```

The specific parameters are described in the following table:

| Entry              | Default | Description                                                                                                                  |
|--------------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `type`             | N/A     | has to be set to `"lua"` (mandatory)                                                                                         |
| `check_after`      | _empty_ | number of seconds that have to pass before the condition is checked the first time or further times if `recurrent` is _true_ |
| `script`           | N/A     | the _Lua_ code that has to be executed by the internal interpreter (mandatory)                                               |
| `expect_all`       | _false_ | if _true_, all the expected results have to be matched to consider the task successful, otherwise at least one               |
| `expected_results` | `{}`    | a dictionary of variable names and their expected values to be checked after execution                                       |

The same rules and possibilities seen for _Lua_ based tasks also apply to conditions: the embedded _Lua_ interpreter is enriched with library functions that allow to write to the **whenever** log, at all logging levels (_error_, _warn_, _info_, _debug_, _trace_). The library functions are the following:

* `log.error`
* `log.warn`
* `log.info`
* `log.debug`
* `log.trace`

and take a single string as their argument. Also, from the embedded _Lua_ interpreter there is a value that can be accessed:

* `whenever_condition` is the name of the condition being checked.

External scripts can be executed via `dofile("/path/to/script.lua")` or by using the `require` function. While a successful execution is always determined by matching the provided criteria, an error in the script is always considered a failure.

For this type of condition the actual test can be performed at a random time within the tick interval.

#### DBus method

The return message of a _DBus method invocation_ is used to determine the execution of the tasks associated to this type of condition. Due to the nature of DBus, the configuration of a _DBus_ based condition is quite complex, both in terms of definition of the method to be invoked, especially for what concerns the parameters to be passed to the method, and in terms of specifying how to test the result[^7]. One of the most notable difficulties consists in the necessity to use embedded _JSON_[^2] in the TOML configuration file: this choice arose due to the fact that, to specify the arguments to pass to the invoked methods and the criteria used to determine the invocation success, _non-homogeneous_ lists are needed -- which are not supported, intentionally, by TOML.

So, as a rule of thumb:

* arguments to be passed to the DBus method are specified in a string containing the _exact_ JSON representation of those arguments
* criteria to determine expected return values (which can be complex structures) are expressed as inline tables of three elements, that is:
  * `"index"`: a list of elements, which can be either integers or strings (the first one is _always_ an integer) representing each a positional 0-based index or a string key in a dictionary; this allows to index deeply nested structures in which part of the nested elements are dictionaries
  * `"operator"`: one of the following strings
    * `"eq"` for _equality_
    * `"neq"` for _inequality_
    * `"gt"` meaning _greater than_
    * `"ge"` meaning _greater or equal to_
    * `"lt"` meaning _less than_
    * `"le"` meaning _less or equal to_
    * `"match"` specified that the second operand has to be intended as a _regular expression_ to be matched
  * `"value"`: the second operand for the specified operator.

Note that not all types of operand are supported for all operator: comparisons (_greater_ and _greater or equal_, _less_ and _less or equal_) are only supported for numbers, and matching is only supported for strings.

A further difficulty is due to the fact that, while DBus is strictly typed and supports all the basic types supported by _C_ and _C++_, neither TOML nor JSON do. Both (and especially JSON, since it is used for invocation purpose in **whenever**) support more generic types, which are listed below along with the DBus type to which **whenever** converts them in method invocation:

* _Boolean_: `BOOLEAN`
* _Integer_: `I64`
* _Float_: `F64`
* _String_: `STRING`
* _List_: `ARRAY`
* _Map_: `DICTIONARY`

This means that there are a lot of value types that are not directly derived from the native JSON types. **whenever** comes to help by allowing to express strictly typed values by using specially crafted strings. These string must begin with a backslash, `\`, followed by the _signature_ character (_ASCII Type Code_ in the basic type table[^8]) identifying the type. For example, the string `"\\y42"` indicates a `BYTE` parameter holding _42_ as the value, while `"\\o/com/example/MusicPlayer1"` indicates an `OBJECT_PATH`[^9] containing the value _/com/example/MusicPlayer1_. A specially crafted string will be translated into a specific value of a specific type _only_ when a supported _ASCII Type Code_ is used, in all other cases the string is interpreted literally: for instance, `"\\w100"` is translated into a `STRING` holding the value _\w100_.

For return values, while the structure of complex entities received from DBus is kept, all values are automatically converted to more generic types: a returned `BYTE` is converted to a JSON _Integer_, and a returned `OBJECT_PATH` is consdered a JSON _String_ which, as a side effect, supports the `"match"` operator.

An example of _DBus_ method based condition follows:

```toml
[[condition]]
name = "DbusMethodConditionName"
type = "dbus"                       # mandatory value
bus = ":session"                    # either ":session" or ":system"
service = "org.freedesktop.DBus"
object_path = "/org/freedesktop/DBus"
interface = "org.freedesktop.DBus"
method = "NameHasOwner"

# optional parameters (if omitted, defaults are used)
recurring = false
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = true
tasks = [ "Task1", "Task2" ]
check_after = 60
parameter_call = """[
        "SomeObject",
        [42, "a structured parameter"],
        ["the following is an u64", "\\t42"]
    ]"""
parameter_check_all = false
parameter_check = """[
         { "index": 0, "operator": "eq", "value": false },
         { "index": [1, 5], "operator": "neq", "value": "forbidden" },
         {
             "index": [2, "mapidx", 5],
             "operator": "match",
             "value": "^[A-Z][a-zA-Z0-9_]*$"
         }
    ]"""
```

As shown below, `parameter_check` is the list of criteria against which the _return message parameters_ (the invocation results are often referred to with this terminology in DBus jargon): for what has been explained above, the checks are performed like this:

1. the first element (thus with 0 as index) of the returned array is expected to be a boolean and to be _false_
2. the second element is considered to be an array, whose sixth element (with index 5) must not be the string _"forbidden"_
3. the third element is highly nested, containing a map whose element with key _"mapidx"_ is an array, containing a string at its sixth position, which should be an alphanumeric beginning with a letter that also can contain underscores (that is, matches the _regular expression_ `^[A-Z][a-zA-Z0-9_]*$`).

Note that the first check shows a `0` index not embedded in a list: if a returned parameter is not an array or a dictionary and its value is required directly, the square brackets around this single index can be omitted and **whenever** does not complain. Since this is probably the most frequent use case, this is a way to make configuration for such cases more readable and concise.

Since `parameter_check_all` is _false_, satisfaction of one of the provided criteria is sufficient to determine the success of the condition.

The specific parameters are described in the following table:

| Entry                 | Default | Description                                                                                                                  |
|-----------------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `type`                | N/A     | has to be set to `"dbus"` (mandatory)                                                                                        |
| `check_after`         | _empty_ | number of seconds that have to pass before the condition is checked the first time or further times if `recurrent` is _true_ |
| `bus`                 | N/A     | the bus on which the method is invoked: must be either `":system"` or `":session"`, including the starting colon (mandatory) |
| `service`             | N/A     | the name of the _service_ that exposes the required _object_ and the _interface_ to invoke query (mandatory)                 |
| `object_path`         | N/A     | the _object_ exposing the _interface_ to invoke or query (mandatory)                                                         |
| `interface`           | N/A     | the _interface_ to invoke or query (mandatory)                                                                               |
| `method`              | N/A     | the name of the _method_ to be invoked (mandatory)                                                                           |
| `parameter_call`      | _empty_ | a structure, expressed as inline JSON, containing exactly the parameters that shall be passed to the method                  |
| `parameter_check_all` | _false_ | if _true_, all the returned parameters will have to match the criteria for verification, otherwise one match is sufficient   |
| `parameter_check`     | _empty_ | a list of maps consisting of three fields each, each of which is a check to be performed on return parameters                |

The value corresponding to the `service` entry is often referred to as _bus name_ in various documents: here _service_ is preferred to avoid confusing it with the actual bus, which is either the _session bus_ or the _system bus_.

Methods resulting in an error will _always_ be considered as failed: therefore it is possible to avoid to provide return value criteria, and just consider a successful invocation as a success and an error as a failure.

Working on a file that mixes TOML and JSON, it is worth to remind that JSON supports inline maps distributed on multiple lines (see the example above, the third constraint) and that in JSON trailing commas are considered an error. Also, JSON does not support _literal_ strings, therefore when using backslashes (for instance when specifying typed values with strings as described above), the backslashes themselves have to be escaped within the provided JSON strings.

Note that DBus based conditions are supported on Windows, however DBus should be running for such conditions to be useful -- which is very unlikely to say the least.

For this type of conditions the actual test can be performed at a random time within the tick interval.

#### Event based conditions

Conditions that are fired by _events_ are referred to here both as _event_ conditions and as _bucket_ conditions. The reason for the second name is that every time that **whenever** catches an event that has been required to be monitored, it tosses the associated condition in a sort of _execution bucket_, that is checked by the scheduler at every tick: the scheduler withdraws every condition found in the bucket and runs the associated tasks. In facts, these conditions only exist as a connection between the events, that occur asynchronously, and the scheduler. Their configuration is therefore very simple, as seen in this example:

```toml
[[condition]]
name = "BucketConditionName"
type = "bucket"         # "bucket" or "event" are the allowed values

# optional parameters (if omitted, defaults are used)
recurring = false
execute_sequence = true
break_on_failure = false
break_on_success = false
suspended = false
tasks = [
    "Task1",
    "Task2",
    ]
```

that is, these conditions have no specific fields, if not for the mandatory `type` that should be either `"bucket"` or `"event"` (with no operational difference, at least for the moment being):

| Entry  | Default | Description                                          |
|--------|:-------:|------------------------------------------------------|
| `type` | N/A     | has to be set to `"bucket"` or `"event"` (mandatory) |

These conditions are associated to _events_ for verification, no other criteria can be specified.

For this type of conditions the actual test can be performed at a random time within the tick interval.


### Events

Only two types of event are supported, at least for now. The reason is that while on Linux DBus handles the majority of the communication between the system and the applications, via a well described subscription mechanism, other environments provide a less portable interface -- even more aimed at usage through APIs that are directly coded in applications. However, in many cases specific checks involving _command_ based conditions can be used to inspect the system status: for example, on Windows the [reg](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/reg) command can be used to inspect the registry, and the [wevtutil](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/wevtutil) command to query the system event log.

One notable exception, which is also particularly useful, is the _notification_ of changes in the filesystem for watched entities (files or directories), which is implemented in **whenever** as one of the possible events that can fire conditions, the other being _DBus signals_ which are generally available on linux desktops (at least _Gnome_ and _KDE_).

Note that if an event arises more that once within the tick interval, it is automatically _debounced_ and a single occurrence is counted.

All _event_ definition sections must start with the TOML `[[event]]` header.

The associated conditions must exist, otherwise an error is raised and **whenever** aborts.

#### Filesystem changes

This type of event arises when there is a modification in the filesystems, regarding one of more monitored files and/or directories. **whenever** allows to monitor a list of items for each defined event of this type, and to associate an _event_ based condition to the event itself. A sample configuration follows:

```toml
[[event]]
name = "FilesystemChangeEventName"
type = "fschange"
condition = "AssignedConditionName"

# optional parameters (if omitted, defaults are used)
watch = [
    "/path/to/resource",
    "/another/path/to/file.txt",
    ]
recursive = false
poll_seconds = 2
```

The configuration entries are:

| Entry              | Default | Description                                                                                |
|--------------------|:-------:|--------------------------------------------------------------------------------------------|
| `name`             | N/A     | the unique name of the event (mandatory)                                                   |
| `type`             | N/A     | must be set to `"fschange"` (mandatory)                                                    |
| `condition`        | N/A     | the name of the associated _event_ based condition (mandatory)                             |
| `watch`            | N/A     | a list of items to be monitored: possibly expressed with their full path                   |
| `recursive`        | _false_ | if _true_, listed directories will be monitored recursively                                |
| `poll_seconds`     | 2       | generally not used, can be needed on systems where the notification service is unavailable |

#### DBus signals

DBus provides signals that can be subscribed by applications, to receive information about various aspects of the system status in an asynchronous way. **whenever** offers the possibility to subscribe to these signals, so that when the associated parameters match any provided constraints the event subscribing to the signal occurs, and the associated condition is fired.

Subscription is performed by providing a _watch expression_ in the same form that is used by the [_dbus-monitor_](https://dbus.freedesktop.org/doc/dbus-monitor.1.html) utility, therefore JSON is not used for this purpose. JSON is used instead to specify the criteria that the _signal parameters_ must meet in order for the event to arise, using the same format that is used for _return message parameter_ checks in _DBus method_ based conditions.

A sample configuration section follows:

```toml
name = "DbusMessageEventName"
type = "dbus"                       # mandatory value
bus = ":session"                    # either ":session" or ":system"
condition = "AssignedConditionName"
rule = """\
    type='signal',\
    sender='org.freedesktop.DBus',\
    interface='org.freedesktop.DBus',\
    member='NameOwnerChanged',\
    arg0='org.freedesktop.zbus.MatchRuleStreamTest42'\
"""

# optional parameters (if omitted, defaults are used)
parameter_check_all = false
parameter_check = """[
         { "index": 0, "operator": "eq", "value": false },
         { "index": [1, 5], "operator": "neq", "value": "forbidden" },
         {
             "index": [2, "mapidx", 5],
             "operator": "match",
             "value": "^[A-Z][a-zA-Z0-9_]*$"
         }
    ]"""
```

and the details of the configuration entries are described in the table below:

| Entry                 | Default | Description                                                                                                                  |
|-----------------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `name`                | N/A     | the unique name of the event (mandatory)                                                                                     |
| `type`                | N/A     | must be set to `"dbus"` (mandatory)                                                                                          |
| `condition`           | N/A     | the name of the associated _event_ based condition (mandatory)                                                               |
| `bus`                 | N/A     | the bus on which the method is invoked: must be either `":system"` or `":session"`, including the starting colon (mandatory) |
| `parameter_check_all` | _false_ | if _true_, all the returned parameters will have to match the criteria for verification, otherwise one match is sufficient   |
| `parameter_check`     | _empty_ | a list of maps consisting of three fields each, each of which is a check to be be performed on return parameters             |

The consideration about indexes in return parameters are the same that have been seen for [_DBus message_ based conditions](#dbus-method).

If no parameter checks are provided, the event arises simply when the signal is caught.


## Logging

Log messages are not dissimilar to the ones provided by servers and other applications running in the background: a date/time specification is reported, as well as the name of the application (_whenever_), the logging level to which the message line is pertinent, and then a message (the so-called _payload_). The message itself is structured: it consists of a short _context_ specification, followed by a string enclosed in square brackets describing the nature of the message (for instance if the message is referred to the start or to the end of a process, and whether the message indicates a normal condition or something that went wrong). The context can be either the _MAIN_ control program (or one of its threads), a _TASK_, a _CONDITION_, an _EVENT_ or a _REGISTRY_ -- there are many registries in **whenever**, used by the main control program to reach the _item_ collections.

Logging is quite verbose in **whenever** at the _trace_ log level, and can be very brief when enabling logging just for warnings and errors.

A short description of the log levels follows:

1. **trace:** every single step is logged, some messages can be redundant because if an acknowledgement or an issue takes place in more than one context of the program, each of the involved parts may decide to log about what happened. Sometimes, for example, the same error may be reported by a condition that is checked and by the registry that has been used to reach this condition. Also, _history_ messages are issued only at the trace level: _wrappers_ will want to use the _trace_ level in order to catch these messages and calculate, for instance, the execution time for a particular task.
2. **debug:** there is plenty of informational messages at each execution step, however redundant messages are not emitted. In particular, _history_ messages are not present at this level.
3. **info:** a reduced amount of informational messages is emitted, mostly related to the outcome of conditions and execution of related tasks; information about what is being checked is less verbose. Very reduced logging is performed at this level by the main control program, thus most of the logging is left to items.
4. **warn:** only **warnings** are logged, erratic situations that can be handled by **whenever** without having to stop or abort -- except for termination requests, which are logged as **warnings** instead of **errors**, even though they could be considered normal causes for the scheduler to stop and exit.
5. **error:** only **errors** are reported, which are erratic situations that may prevent **whenever** to perform the requested operations or, in some cases, to keep running correctly.

Note that, since _Lua_ scripts are allowed to log at each of the above described levels, lines emitted by _Lua_ script might not always correspond to what is illustrated above.

As mentioned above, just after the _context_, in the message _payload_, a string of this form `[NATURE/OUTCOME]` appears that can be used to identify the nature of the message, where

* _NATURE_ is one of
  * `INIT` when the message is related to an initialization phase (mostly around startup)
  * `START` when the message is issued when _starting_ something, for instance a check or a new process
  * `PROC` when the message is issued in the middle of something, for instance while executing a check
  * `END` when the message is emitted at the end of something, before returning control
  * `HIST` when the message is intended for some receiver (generally a wrapper) that keeps track of the history: in this case the _outcome_ is either `START` or `END`

* _OUTCOME_ is one of the following:
  * `OK` for expected behaviours
  * `FAIL` for unexpected behaviours
  * `IND` when the outcome of an operation is undetermined
  * `MSG` when the message is merely informational
  * `ERR` (possibly followed by `-nnn` where nnn is a code that should be notified), when an operation fails with a known error
  * `START`/`END` are pseudo-outcomes that only occur when the _nature_ is `HIST`, to mark the beginning or the end of an activity

This string appears _before_ a human-readable message, so that it can be used by a wrapper to filter or highlight message when displaying the log -- completely or partially. Sometimes it might seem that the expression in square bracket conflicts with the message body, a notable example being a message similar to

```text
[2023-06-20T21:53:45.089] (whenever) INFO  CONDITION Cond_INTERVAL/[6]: [PROC/OK] failure: condition checked with negative outcome
```

while in fact this kind of message is absolutely legitimate: a negative outcome in condition checking is expected quite often, this is the reason why the message documenting a failed check is reported as a positive (`[PROC/OK]`) log entry.

There is an option that can be specified on the [command line](#cli), that forces the log lines to be emitted in the JSON format: this allows to separate the parts more easily in

* timestamp
* application name
* log level
* message (payload)

in order to better handle the logs and to provide feedback to the user.


## Conclusion

The configuration of **whenever** might be difficult, especially for complex aspects such as events and conditions based on DBus. In this sense, since **whenever** does not provide a GUI, the features of the Python based **When** are not completely matched. However, this happens to be a significant step towards solution of [issue #85](https://github.com/almostearthling/when-command/issues/85) in the Python version. Moreover, **whenever** gains some useful features (such as the _Lua_ embedded interpreter) in this transition, as well as the possibility of running on many platforms instead of being confined to a restricted number of versions of Ubuntu Linux, and the very low impact on the system in terms of resource usage.

I am considering **whenever** as the evolution of the **When** operational engine, and future versions of **When** itself (which will bump its version number to something more near to the awaited _2.0.0_) will only implement the GUIs that might (or might not) be used to configure **whenever** and to control it from the system tray in a more sophisticated way than the one allowed by the minimal C++ based utility.


## License

This tool is licensed under the LGPL v2.1 (may change to LGPL v3 in the future): see the provided LICENSE file for details.


[^1]: Although DBus support is available on Windows too, it is mostly useful on Linux desktops.
[^2]: Because TOML is sometimes too strict and is not able to represent certain types of structured data, [JSON](https://www.json.org/) is used in some cases within the TOML configuration file.
[^3]: When run alone, with no wrapper: using the minimal provided wrapper, both programs together use less than 4MB of RAM and the combined CPU consumption in rare occasions reaches the 0.2% -- as reported by the Windows _Task Manager_.
[^4]: The occurrence of an _event_, in fact, raises a flag that specifies that the associated condition will be considered as verified at the following tick: the condition is said to be thrown in a sort of "execution bucket", from which it is withdrawn by the scheduler that executes the related tasks. Therefore _event_ based conditions are also referred to as _bucket_ conditions.
[^5]: In this case the execution will continue as long as the outcome is _undefined_ until the first success or failure happens.
[^6]: Except on _Wayland_ based Linux systems (e.g. Ubuntu), on which the idle time starts _after the session has been locked_.
[^7]: In fact, in the original **When** the DBus based conditions and events were considered an advanced feature: even the dialog box that allowed the configuration of user-defined DBus events was only available through a specific invocation using the command line.
[^8]: See the [DBus Specification](https://dbus.freedesktop.org/doc/dbus-specification.html#basic-types) for the complete list of supported types, and the ASCII character that identifies each of them.
[^9]: in DBus, strings and object paths are considered different types.
