# The Whenever Task Scheduler

![HeaderImage](docs/graphics/banner.png)

> _This is not the scheduler you are looking for..._

[![Linux Standard Build](https://github.com/almostearthling/whenever/actions/workflows/rust-linux-std.yml/badge.svg)](https://github.com/almostearthling/whenever/actions/workflows/rust-linux-std.yml)
[![Windows Standard Build](https://github.com/almostearthling/whenever/actions/workflows/rust-windows-std.yml/badge.svg)](https://github.com/almostearthling/whenever/actions/workflows/rust-windows-std.yml)

[![Documentation](https://github.com/almostearthling/whenever/actions/workflows/documentation.yaml/badge.svg)](https://github.com/almostearthling/whenever/actions/workflows/documentation.yaml)


**whenever** is a lightweight automation tool capable of executing _tasks_ when specific _conditions_ are verified. Conditions can be of several types, for example:

* :alarm_clock: time based, that is, verified at intervals or specific more-or-less defined instants,
* :wrench: depending on the results of OS commands or _Lua_ scripts,
* :computer: based on the inspection of system properties, via _DBus_ on Linux and _WMI_ on Windows,
* :bomb: reactions to _events_, such as:
  * :file_folder: changes in specific files and directories,
  * :zzz: session inactivity,
  * :rotating_light: _DBus_ signals on Linux, and _WMI_ event queries on Windows.

while _tasks_ mostly consist in the execution of OS commands and _Lua_ scripts. This is done within a desktop session, without the need for the user to have administrative rights.


## :sparkles: Purpose

**whenever** works on both Linux and Windows: on Linux it allows for a more flexible way of automating actions compared to the traditional _cron_ method, and on Windows it offers a more streamlined way to schedule activities, compared to the system-provided _Time Scheduler_ which is available through the system management console.

The ability to inspect [_DBus_](https://www.freedesktop.org/wiki/Software/dbus/) and [_WMI_](https://learn.microsoft.com/it-it/windows/win32/wmisdk/wmi-start-page) as well as react to the respective signals and events, and to use system commands to check their status and output, allows for conditions to be activated virtually at every possible change in the system status.

The tool intends to remain as frugal as possible in terms of used computational resources, and to possibly run at a low priority level, while still providing high flexibility and configurability. The configuration is provided by a [TOML](https://toml.io/) file, which must contain all definitions for conditions and associated tasks, as well as events that the application should listen to while running in the background.


## :floppy_disk: Installation

Even though **whenever** is a console application, it is designed for desktops: therefore it should be executed via a controlling GUI frontend. Currently, there are two companion wrappers available:

* [When](https://github.com/almostearthling/when-command), a Python based fully featured application to configure and run **whenever**,
* [whenever_tray](https://github.com/almostearthling/whenever_tray), a minimal wrapper that displays an icon in the system tray and provides basic interaction.

Prebuilt binaries can be downloaded from the [releases](https://github.com/almostearthling/whenever/releases) page, and basic [installation instructions](https://almostearthling.github.io/whenever/90.install.html) are provided in the online documentation. However, the easiest (and suggested) way to get **whenever** up and running, is to [install When](https://almostearthling.github.io/when-command/install.html), use it to download and configure **whenever**, and set it up to start when the user session begins.

The provided binaries are self-contained on Windows, and almost so on Linux: on Linux they require the _X.org_ subsystem to be installed, a prerequisite that is often satisfied. So, to just give a try to the console application you can:

1. download the prebuilt binary archive suitable for your OS, and extract the **whenever** executable to a directory in your PATH, for example _~/.local/bin_ if present
2. create and edit a file named _whenever.toml_, so that it contains the following text:

   ```toml
   [[task]]
   type = "lua"
   name = "TRACE"
   script = '''log.warn("Trace: *** VERIFIED CONDITION *** `" .. whenever_condition .. "`");'''

   [[condition]]
   name = "Periodic_15s"
   type = "interval"
   interval_seconds = 15
   recurring = true
   tasks = ["TRACE"]
   ```

3. launch the following command, in the same directory where _whenever.toml_ is located:

   ```shell
   whenever -L trace whenever.toml
   ```

The output should be similar to the following:

[![asciicast](https://asciinema.org/a/2q7yy5p1uqv9FBRRGl53LvZUb.svg)](https://asciinema.org/a/2q7yy5p1uqv9FBRRGl53LvZUb)

Well, not that impressive... But, in fact, **whenever** has not been designed to be used this way.

To terminate the console application, just hit `Ctrl+C` and it will gracefully stop.


## :book: Documentation

Detailed [documentation](https://almostearthling.github.io/whenever/index.html) is available, which explains how to configure **whenever** by manually editing its configuration file. However, the installation and the configuration via the **When** frontend are definitely easier, and the most useful resources are in this case the following:

* [Installation](https://almostearthling.github.io/when-command/install.html)
* [Tutorial](https://almostearthling.github.io/when-command/tutorial.html)
* [Manual](https://almostearthling.github.io/when-command/main.html)

**When** is also helpful to instruct **whenever** to start at the beginning of a session.

Note that the documentation generally refers to the version of **whenever** currently in the _main_ branch, that is, the latest stable version: it is usually published as a release as well, although sometimes the latest published release might still be some steps behind. Please check the version that the documentation refers to on the [index](https://almostearthling.github.io/whenever/index.html) page.


## :radioactive: Issues and Breaking Changes

**Breaking change:** The _0.4_ series of **whenever** introduces an incompatibility in terms of configuration file with early versions of the _0.3_ series, as it ultimately drops support for embedded JSON strings to define parameter checks for _DBus_: using JSON has been deprecated in version _0.3.8_ (a log warning is issued if JSON is found), in favor of pure TOML dictionaries. Pure TOML is obviously easier to read and manage especially using small inline tables, one for each check, and JSON support is dropped in order to reclaim some memory.


## :lady_beetle: Bug Reporting

If there is a bug in the **whenever** application, please use the project [issue tracker](https://github.com/almostearthling/whenever/issues) to report it.


## :balance_scale: License

**whenever** is released under the terms of the [LGPL v2.1](LICENSE).
