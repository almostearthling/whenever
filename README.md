# The Whenever Task Scheduler

![HeaderImage](docs/graphics/metronome.png)


**whenever** is a lightweight scheduler and automation tool capable of executing _tasks_ (in particular, OS commands and _Lua_ scripts) according to specific _conditions_. Conditions can be of several types:

* :alarm_clock: time based, that is, verified at intervals or specific more-or-less defined instants,
* :wrench: depending on the results of OS commands or _Lua_ scripts,
* :computer: based on the inspection of system properties, via _DBus_ on Linux and _WMI_ on Windows,
* :bomb: reactions to _events_, such as:
  * :file_folder: changes in specific files and directories,
  * :zzz: session inactivity,
  * :rotating_light: _DBus_ signals on Linux, and _WMI_ event queries on Windows.

and more. The ability to inspect [_DBus_](https://www.freedesktop.org/wiki/Software/dbus/) and [_WMI_](https://learn.microsoft.com/it-it/windows/win32/wmisdk/wmi-start-page), and to react to signals and events, and to use system commands and check their status and output, allows for conditions to be triggered by virtually every possible change in the system status.

The scheduler intends to remain as frugal as possible in terms of used computational resources, and to possibly run at a low priority level, while still providing high flexibility and configurability. The configuration is provided by a [TOML](https://toml.io/) file, which must contain all definitions for conditions and associated tasks, as well as events that the scheduler should listen to while running in the background.

Even though **whenever** is a command line application, it is designed for desktops: therefore it should be executed via a controlling GUI frontend. Currently, there are two companion wrappers available:

* [When](https://github.com/almostearthling/when-command), a Python based fully featured application to configure and run **whenever**,
* [whenever_tray](https://github.com/almostearthling/whenever_tray), a minimal wrapper that displays an icon in the system tray and provides basic interaction.

Prebuilt binaries can be downloaded from the [releases](https://github.com/almostearthling/whenever/releases) page, and basic [installation instructions](https://almostearthling.github.io/whenever/90.install.html) can be found in the online documentation. However, the easiest (and suggested) way to get **whenever** up and running, is to [install When](https://almostearthling.github.io/when-command/install.html), use it to download and configure **whenever**, and set it up to start when the user session begins.

**whenever** is released under the terms of the [LGPL v2.1](LICENSE).

Please refer to the [documentation](https://almostearthling.github.io/whenever/index.html) for configuration and installation details.


[![pages-build-deployment](https://github.com/almostearthling/whenever/actions/workflows/pages/pages-build-deployment/badge.svg)](https://github.com/almostearthling/whenever/actions/workflows/pages/pages-build-deployment)
