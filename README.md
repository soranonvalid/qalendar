# qalendar

qalendar is a desktop calendar for planning activities by date and time. Use the month view to see what’s coming up, then switch to Time view to see the selected day on a zoomable horizontal timeline. Timed activities appear as colored blocks whose widths show their duration.

- Select a date and click **+** to add an activity. Start and end times are optional.
- Drag an activity onto another date in the month view to reschedule it.
- Drag an activity from the sidebar onto the time ruler to set its start time. If it already has a duration, the duration stays the same.

## Install from source on Windows

1. Install [Git for Windows](https://gitforwindows.org/) and the [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/). In the Visual Studio Installer, select **Desktop development with C++** and include the Windows SDK.
2. Install [Rust using rustup](https://rustup.rs/). Accept the default stable MSVC toolchain, then open a new PowerShell window.
3. Clone and run qalendar.

   ```powershell
   git clone https://github.com/soranonvalid/qalendar.git
   cd qalendar
   cargo run --locked
   ```

   The first build downloads dependencies. After it finishes, you can launch `target\debug\qalendar.exe` directly.

## Local data

qalendar keeps your activities on your computer. New installations use `%LOCALAPPDATA%\qalendar\activities.json`. Keep a copy of that file if you want to back up your calendar.
