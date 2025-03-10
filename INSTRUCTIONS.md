# Instructions

## Quick Installation

On Linux, the binary _wxGTK_ distribution should be installed: use your favorite package manager to install _wxGTK 3.2_ or later. Also, libraries from the X11 distribution are required, that is _libxss_ and _libX11_, which normally come along with _X11_ support. App indicators should be manually enabled on most modern GNOME distributions, by installing an appropriate shell extension (see **whenever_tray** [documentation](https://github.com/almostearthling/whenever_tray/blob/main/README.md)). On Windows, everything should work _out of the box_.

1. Download the binaries for your architecture, and unzip the contents in a directory included in the system _PATH_
2. Create the _application data directory_ for the GUI wrapper:

    - on Linux: `mkdir ~/.whenever`
    - on Windows: `mkdir %APPDATA%\Whenever`

3. Edit _whenever.toml_ in the _application data directory_, so that it contains the following text:

    ```toml
    # whenever configuration file
    scheduler_tick_seconds = 5
    randomize_checks_within_ticks = true

    [[task]]
    type = "lua"
    name = "TRACE"
    script = '''
        log.warn("Trace: *** VERIFIED CONDITION *** `" .. whenever_condition .. "`");
        '''

    [[condition]]
    name = "Periodic00"
    type = "interval"
    interval_seconds = 60
    recurring = true
    tasks = [
        "TRACE",
        ]

    # end.
    ```

4. Edit _whenever_tray.toml_ in the _application data directory_, so that it contains the following text (uncomment _logview_command_ as appropriate for your system):

    ```toml
    [whenever_tray]
    # path to the text processor used to view the log file
    #logview_command = 'gnome-text-editor'
    #logview_command = 'gedit'
    #logview_command = 'notepad.exe'
    ```

5. Launch **whenever_tray**, for example from the command line
6. Access **whenever** from the metronome icon in the notification/tray area.

The _whenever.toml_ configuration file should be edited to suit your needs (see [README.md](https://github.com/almostearthling/whenever/blob/main/README.md) for details). The sample configuration only logs a line (with _WARN_ severity) every minute. Also, you might want to install a log viewer application to specify as _logview_command_ in _whenever_tray.toml_ instead of a simple editor as shown in the provided examples.

The **whenever_tray** utility must be stopped/restarted to load the new configuration, both from _whenever.toml_ and _whenever_tray.toml_, once you modify them.


## Installation using the When wrapper

At the moment the easiest way to install **whenever** and start using it effectively is through its full-featured frontend: [**When**](https://github.com/almostearthling/when-command). You can follow its specific installation [instructions](https://github.com/almostearthling/when-command/blob/main/support/docs/install.md): **When** takes care of downloading and checking the latest binary release of **whenever**, and provides a way to install the program icons and to autostart the scheduler upon login. Moreover, **When** offers a graphical UI to configure _tasks_, _conditions_, and _events_, instead of manually editing the configuration file.


## Compatibility

The binaries have been tested on:

- Windows 10
- Windows 11
- Debian 12 Linux

The Linux distributions have been chosen in order to cover different base systems. Other distributions should work, provided that the required libraries/shell extensions are available, and that an _Xorg_ session is accessible: Linux binaries might not work correctly in _Wayland_ sessions, see the documentation for both **whenever** and **whenever_tray** for details. To use the bundle it is safer to logon in an _Xorg_ session.

