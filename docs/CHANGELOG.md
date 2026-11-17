# Changelog

All notable changes to the Wintil project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Fake Fullscreen Mode (`src/window/fullscreen.rs`, `src/main.rs`)**:
  - Added borderless work-area fullscreen toggle (`Alt + Shift + F` / `hotkeys.fake_fullscreen`) via `FakeFullscreenStore`, stripping window chrome, suppressing Windows 11 DWM border lines and corner rounding, and sizing windows to `rcWork` (`height - 1px`) to keep the taskbar accessible without triggering DWM fullscreen occlusion / Direct Flip.

### Fixed
- **Fake Fullscreen Desync, Stale Surface Rendering & Taskbar Overlap (`src/window/fullscreen.rs`, `src/main.rs`)**:
  - Fixed a bug where toggling fake fullscreen on windows spawned extending below the taskbar or toggling between maximize (`Alt + F`) and fake fullscreen (`Alt + Shift + F`) left a frozen/dead bottom strip (e.g., in Windows Terminal / XAML Island apps) or placed windows under the taskbar.
  - Pre-maximizes non-maximized windows (`ShowWindow(SW_MAXIMIZE)` + `SendMessageTimeoutW(WM_NULL)`) with chrome intact before stripping styles so XAML/DirectComposition child surfaces reflow cleanly to `rcWork` while preserving the original pre-fullscreen `WINDOWPLACEMENT`.
  - Strips `WS_MAXIMIZE | WS_MINIMIZE` without mutating `WS_OVERLAPPED` windows to `WS_POPUP`, passes `SWP_NOCOPYBITS` and `RedrawWindow`, commits `SWP_FRAMECHANGED` before `SetWindowPlacement` on restore, skips already-borderless native fullscreen windows, and exits fake fullscreen before `ToggleMaximize`.

---

## [0.3.0] - 2026-11-15

### Added
- **Interactive Runtime Keybinds Configuration Editor (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Added a dedicated, full-panel Keybind Configuration editor (`paint_keybinds_editor`) accessible via the `[ Edit Keybinds ]` button at the bottom of the Keybinds subview.
  - Lists all 10 system actions with active key combo badge pills and interactive `[ Rebind ]` buttons.
  - Includes `[ Reset to Defaults ]` and `[ Done ]` action buttons for effortless configuration management.
  - Runtime hotkey re-registration (`app_update_hotkey`): updates to bindings are immediately saved to `wintil.toml` and registered with the Windows subsystem on the fly without requiring an application restart.
- **Interactive Keybind Rebind Modal Dialog & Hook Suspension (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Implemented an in-flyout modal dialog with a dimmed semi-transparent backdrop overlay powered by `GdiAlphaBlend`.
  - Automatically suspends/unregisters active global hotkey hooks (`app_suspend_hotkeys`) when the rebind modal opens so pressing existing key combinations during remapping does not trigger window actions, and resumes hooks (`app_resume_hotkeys`) on Reset, Confirm, Escape, or dismissal.
  - Displays a clean prompt `"Use:"` above a high-contrast white input box showing the real-time recorded keyboard combination in dark charcoal text, and target action descriptor `"for: [shortcut]"` below.
  - Detects conflicting keymappings in real time via canonical `(modifiers, vk)` comparison (`find_hotkey_conflict` / `app_check_hotkey_conflict`): pressing an already-bound combination displays a bright red error message (`"Conflict: Already bound to '<Action>'"`), tints the input box border and text red, and prevents `[ Confirm ]` or `Enter` from committing the conflicting mapping.
- **Strict Mouse Input Filtering for Keyboard Shortcuts (`src/config/keys.rs`, `src/flyout/mod.rs`)**:
  - Explicitly disallowed mouse inputs from being parsed or assigned as global keyboard shortcuts, blocking `mousekeys1`, `mousekeys2`, `mwheel_down`, `mwheel_up`, and virtual keys `VK_LBUTTON`, `VK_RBUTTON`, `VK_MBUTTON`, `VK_XBUTTON1`, `VK_XBUTTON2`.
  - Intercepted and dropped `WM_MOUSEWHEEL` and `WM_MOUSEHWHEEL` events while the rebind modal is active.
- **Scrollable Container Engine & Universal List Scrolling (`src/flyout/layout.rs`, `src/flyout/mod.rs`)**:
  - Added `ScrollView`, `ScrollViewBuilder`, and `ScrollState` to the Win32 GDI layout engine supporting unconstrained inner content measurement, viewport offset clamping, `WM_MOUSEWHEEL` delta stepping, proportional scrollbar thumb calculation, `SaveDC` / `IntersectClipRect` / `RestoreDC` GDI viewport clipping, and automatic clipping of click targets and tooltips to visible viewport bounds.
  - Added `ScrollRegistry` with nested scroll chaining across **Window Ops**, **Subservices**, **Automations**, **Keybinds**, **Auto-Hide Rules**, and **Automation Editor**.
- **Automation Editor Inline Editing, Conflict Guard & File Picker (`src/flyout/mod.rs`, `src/automations/mod.rs`, `src/main.rs`)**:
  - Added inline-editable **Name**, **Desc**, and conditional **Subservice Metadata** (`Title`) fields directly inside the Automation Editor.
  - Added real-time name/folder conflict detection (`find_automation_conflict` / `app_check_automation_conflict`) with red error feedback and disabled Save to prevent overwriting or deleting existing automation files.
  - Added native Windows File Explorer script picker (`[ Browse... ]` via `GetOpenFileNameW`) that clones the selected script into `$HOME\.config\wintil\automations\<name>\` on save.
  - Moved **Enable Automated Execution** into **Event Triggers** and added a collapsible **Only Manual Execution** toggle.
- **Crash Sentry Live Error Escalation & GUI Session Tracking (`src/sentry/mod.rs`, `src/flyout/mod.rs`, `src/automations/mod.rs`)**:
  - Wired non-zero automation script exits and spawn failures to `sentry::log_error` (bypassing the 2s debounce for `ERROR` events) and made `invalidate_flyout()` thread-safe via `FLYOUT_HWND`.
  - Added colored `Respawn: on` (green) / `off` (red) subtext and 4-step error escalation (`!` yellow → `!` red → `!!!` red → `Critical Errors!` red) that clears automatically on the second GUI reopen when no new errors occurred (`on_gui_opened`).
- **Keep GUI Open Toggle & Exit Confirmation Modal (`src/flyout/mod.rs`)**:
  - Added **Keep GUI Open** toggle in **Settings → System & Application** to keep the flyout open on focus loss until dismissed via tray click or `Esc`.
  - Added an interactive confirmation modal (`draw_exit_confirmation_modal`) when clicking `[ Exit Wintil ]` in Settings.

### Changed
- **Windows-Native Service Terminology & UI Standardization (`src/main.rs`, `src/automations/mod.rs`, `src/config/mod.rs`, `src/hider/daemon.rs`, `src/flyout/mod.rs`)**:
  - Replaced Linux "daemon / subdaemon" terminology across UI, structs, and serialized configuration with Windows-focused **"service / subservice"** (`AutoHiderService`, `ServiceSnapshotInfo`, `SubserviceSnapshotInfo`, `service_interval_ms`, `subservice`, `is_subservice`), while preserving serde aliases (`daemon_interval_ms`, `subdaemon`, `is_subdaemon`) for backward compatibility.
  - Renamed "Auto-Hide Daemon" to **"Auto-Hide Service"** across all flyout menus, options, rules editor, and status indicators.
  - Replaced legacy "cloaked apps / cloaked windows" terminology with **"hidden windows / hidden"** across all user-facing labels and views (`WindowOpsViewMode::Hidden`).
  - Styled the Key Remap UI components (Keybinds submenu pills, Keybind Configuration editor pills, and Rebind Modal) with a neutral dark charcoal / slate palette (`rgb(28, 28, 33)` to `rgb(56, 56, 64)`) and subtle low-contrast borders, while preserving the classic blue active theme (`rgb(45, 110, 220)` fill, `rgb(70, 140, 240)` border) on the main navigation tabs (`Services`, `Options`, `Settings`) and their submenus.
  - Widened the keybind pills in the Keybinds submenu (`28..208`, 180px wide) so multi-modifier shortcuts fit in full, aligned all card margins to `x: 28..392`, and added uniform horizontal text padding with `DT_END_ELLIPSIS` across pills and buttons.
- **Window Ops & Wintil Services Redesign (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Removed the Window Ops tertiary selector and unified **Pinned Windows**, **Pin Border & Indicator Options**, **Hidden Windows** (with **Restore Last / Restore All / Clear List** below the list), and dynamic-height **Auto-Hide Service Rules** into a single scrollable view.
  - Renamed **Native Services** to **Wintil Services**, ordered services as **Key Remapper**, **Crash Sentry**, **Automations Engine**, **Window Pinner**, and **Window Hider**, and replaced hardcoded subtexts with live runtime state.
- **Tooltips, Branding & Tray Simplification (`src/config/mod.rs`, `src/flyout/mod.rs`, `src/main.rs`, `src/tray/mod.rs`)**:
  - Changed `show_tooltips` default to `false`, doubled the hover dwell delay to `900 ms`, restricted tooltips to interactive buttons, and styled tooltips in charcoal/silver matching the UI palette.
  - Removed the redundant tray right-click context menu (`tray/menu.rs`), updated the embedded tray icon fallback to `assets/icon.ico`, removed "Companion" branding across UI/config/docs, and simplified the Settings footer to `Wintil, v<version>`.

### Removed
- Legacy tray right-click context menu (`src/tray/menu.rs`).
  - Removed the legacy Emergency Key Recovery unstuck buffer component from the Keybinds tab to reclaim vertical space.
  - Expanded the Global Keyboard Shortcuts legend into a spacious 10-entry card with 35px row spacing, reading live bindings from `snap.hotkeys`.

---

## [0.2.8] - 2026-11-15

### Added
- **Dedicated Keyboard Shortcuts Submenu (`Keybinds`) (`src/flyout/mod.rs`)**:
  - Extracted keyboard shortcuts out of the Settings tab into its own dedicated, rich subview under Options (`OptionsSubView::Keybinds`).
  - Organized into three clean cards:
    - Global Keyboard Shortcuts: 9 global hook hotkeys formatted with styled key combo badge pills (`[Alt + P]`, `[Alt + Shift + H]`, etc.), action titles, descriptions, and on-hover detail tooltips.
    - Emergency Key Recovery: Hardware buffer flush card featuring an interactive `[ Unstick Modifier Keys ]` button and real-time buffer readiness status.
    - Hotkey Customization: Remapping reference with direct shortcuts to open `wintil.toml` and hot-reload bindings.
- **Unified `Window Ops` Submenu with Expandable View Modes (`src/flyout/mod.rs`)**:
  - Merged Pinned Windows and Cloaked/Hidden Windows into a unified `Window Ops` subview (`OptionsSubView::WindowOps`).
  - Reordered Options subview tabs into the exact requested order: **`Keybinds`** (left), **`Window Ops`** (middle), and **`Automations`** (right).
  - Implemented an intuitive 3-way segmented switcher at the top of Window Ops: `[ Combined ]`, `[ Pinned (N) ]`, and `[ Cloaked (M) ]`.
  - In `Combined` mode, renders compact Pinned and Cloaked windows cards simultaneously, each equipped with an `[ Expand ↗ ]` button to instantly maximize either card over the other.
  - Added `[ ↙ Combined ]` buttons to both expanded views to easily return to the combined dashboard.
  - Compacted the Window Overlays & Rules card: removed the redundant `[ Test Badge ]` button and converted the Pin HUD toggle into a compact button (`[ ✓ Pin Badge: ON ]`), saving significant vertical space.
- **Full-Menu Takeover Auto-Hide Rules Editor (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Implemented a full flyout takeover mode (`paint_autohide_rules_editor`) triggered from the `[ Manage Rules ⚙ ]` button in Window Ops, mirroring the automation editor experience.
  - Active Rules Card: lists all currently registered background auto-hide target executables with immediate `[ Remove ✕ ]` buttons (`FlyoutAction::RemoveAutoHideRule`).
  - Running Desktop Applications Card: dynamically detects and lists active desktop applications with live status (`✓ Active` or `+ Auto-Hide`), enabling instant one-click rule registration without editing config files.
  - Direct shortcut to open `wintil.toml` in Notepad for specifying arbitrary custom executable or path targets.
  - Dismissal via `< Back`, `[ Done ]`, or pressing `VK_ESCAPE`.
- **Chromium Fork & Process Disambiguation Engine (`src/window/process.rs`, `src/hider/daemon.rs`)**:
  - Implemented deep process image inspection (`QueryFullProcessImageNameW`) via `ProcessDetails` in `src/window/process.rs` to distinguish between Chromium browsers and apps that share the binary name `chrome.exe` (e.g., Helium browser installed under `imput\Helium` vs Google Chrome installed under `Google\Chrome`).
  - Added smart path-aware target matching in `matches_auto_hide_target`: rules for `"chrome.exe"` or `"chrome"` strictly match Google Chrome and never cloak Helium, while rules for `"Helium"` strictly match Helium.
  - Preserved support for full filesystem paths as exact auto-hide rules.
- **Decoupled Crash Sentry & Auto-Respawn Supervisor (`src/sentry/mod.rs`, `src/main.rs`)**:
  - Expanded the secondary sentry process (`wintil.exe --sentry <master_pid>`) into a decoupled crash supervisor and telemetry engine that lives completely outside the master process and survives unhandled panics, fast-fails (`0xC0000409`), and access violations (`0xC0000005`).
  - Holds limited query and synchronization rights with zero CPU footprint (`WaitForSingleObject(h, INFINITE)`).
  - On abnormal termination, sentry inspects the process exit code, runs disaster rollback on cloaked and pinned windows, and automatically respawns a healthy master process instance if enabled.
  - Implemented an emergency circuit breaker (sliding window: 3 crashes within 60s) to permanently halt automatic respawning in case of persistent startup crashes, preventing infinite process thrashing.
  - Sentry and crash history state persists across invocations in `wintil_crash_history.toml`.
- **Structured Dual-Format Diagnostic Telemetry Logger (`src/sentry/mod.rs`)**:
  - Implemented a dual-format logger writing human-readable crash reports to `wintil_crash.log` and structured telemetry events to `wintil_events.jsonl` with timestamps, severity levels, component tags, and optional diagnostic details.
  - Built-in NTSTATUS crash code decoding translating raw hexadecimal exit codes (e.g. `0xC0000005: STATUS_ACCESS_VIOLATION`, `0xC0000409: STATUS_STACK_BUFFER_OVERRUN`, `0xC00000FD: STATUS_STACK_OVERFLOW`, `0xE06D7363: STATUS_CPP_EXCEPTION`).
  - Rate-limiting and deduplication filter (2-second window) preventing telemetry ballooning during repetitive failures.
  - Configurable via `[sentry]` in `wintil.toml` with `logging_enabled = true` and `respawn_on_crash = true`.
- **In-App Telemetry & Crash Watchdog Management (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Added a dedicated **Crash Watchdog & Telemetry** card to the Settings tab featuring:
    - Auto-restart toggle: `[ Respawn: ON ]` / `[ Respawn: OFF ]` (`FlyoutAction::ToggleSentryRespawn`).
    - Diagnostic logging toggle: `[ Logger: ON ]` / `[ Logger: OFF ]` (`FlyoutAction::ToggleSentryLogging`).
    - Real-time diagnostic footprint and error counter: `Logs: <size> KB (<count> recorded errors)`.
    - One-click log purging: `[ Clear Logs ]` (`FlyoutAction::ClearLogs`) which deletes all log files and resets the emergency circuit breaker.
  - Integrated a live `[ Clear Logs ]` quick-action button directly into the **Crash Sentry Watchdog** entry in the Services tab.
  - Status pills dynamically reflect `● Active` (green), `○ Standby` (amber/dim), and `✕ Tripped` (alert red) when the circuit breaker trips.
- **Interactive Border Stroke Color Switcher (`src/main.rs`, `src/flyout/mod.rs`, `src/config/mod.rs`)**:
  - Added real-time border color cycling (`FlyoutAction::CycleStrokeColor`) directly in the Pinned Windows options subview.
  - Interactive swatch button cycling through curated preset colors: Burgundy (`#80142A`), Crimson (`#DC2626`), Emerald (`#10B981`), Cyan (`#06B6D4`), Royal Blue (`#2563EB`), Amber Gold (`#F59E0B`), Violet (`#8B5CF6`), and White (`#FFFFFF`).
  - Immediately updates active Win32 stroke overlay windows and persists preference to `wintil.toml`.
- **Declarative Win32 GDI Layout Engine (`src/flyout/layout.rs`)**:
  - Designed and implemented a lightweight declarative layout system for Win32 GDI flyouts.
  - Features composable layout nodes (`Container`, `Card`, `VStack`, `HStack`, `Label`, `Button`, `StatusPill`, `Divider`, `Spacer`).
  - Two-pass architecture: scale-aware `measure()` pass with GDI `DrawTextW` (`DT_CALCRECT`) text metrics and deterministic headless test fallback, followed by a `layout()` pass calculating exact bounding coordinates.
  - Automatically binds interactive `ClickTarget` and `TooltipHit` regions directly to computed physical bounds during render, eliminating magic numbers, manual coordinate arithmetic, and coordinate desync bugs.
  - Added unit test suite verifying `LayoutRect`, `EdgeInsets`, `VStack` vertical stacking, and `HStack` flex distribution.
- **Permanent Delete Confirmation Modal (`src/flyout/mod.rs`)**:
  - Implemented an in-window modal confirmation dialog (`draw_delete_confirmation_modal`) to protect users against accidental deletion of scheduled automation tasks.
  - Features dark crimson styling (`rgb(32, 24, 28)`) with glowing red danger border (`rgb(239, 68, 68)`), permanent action warning badge, highlighted task name card, and balanced `[ Cancel ]` / `[ Confirm Delete ]` action buttons.
  - Supports backdrop dismissal (clicking anywhere outside the modal dialog cancels) as well as keyboard Escape cancellation (`VK_ESCAPE`).
  - Integrated into both the Automations subview header (`[ Delete ]`) and the Full-GUI Automation Editor (`[ Delete ]`).

### Changed & Fixed
- **Unwanted Auto-Hider Registration on Hide-To-Tray Hotkey (`src/main.rs`)**:
  - Fixed an issue where pressing `Alt+Shift+H` (`HotkeyAction::HideToTray`) dynamically added the active window's executable name to `dynamic_auto_hide_exes` and spun up the background `AutoHiderDaemon`.
  - Resolved process conflation where concealing Google Chrome via hotkey would subsequently auto-hide Helium browser in the background because both shared the binary name `chrome.exe`.
  - Refactored `HideToTray` to strictly cloak the focused foreground window to the tray without altering background auto-hide rules.
- **Process Disambiguation for Chromium Forks Sharing Binary Names (`src/window/process.rs`, `src/hider/daemon.rs`)**:
  - Helium browser and Google Chrome both use the executable filename `chrome.exe`. Previously, Wintil stripped full path directories and only matched against `file_name()`, treating both as identical.
  - Implemented `ProcessDetails` in `src/window/process.rs` capturing canonical executable path via `QueryFullProcessImageNameW`, display name (e.g. `Helium (chrome.exe)` vs `Google Chrome (chrome.exe)`), and target rule identifiers.
  - Added smart target matching in `src/hider/daemon.rs`: auto-hide rules targeting `"chrome.exe"` or `"chrome"` strictly match Google Chrome and never cloak Helium, while rules targeting `"Helium"` strictly match Helium.
- **Dedicated Log Master Directory & Sequential Master Log (`src/config/mod.rs`, `src/sentry/mod.rs`, `src/automations/mod.rs`, `src/hotkeys/audit.rs`)**:
  - Established a dedicated log master directory at `$HOME/.config/wintil/logs/` accessed via `crate::config::get_logs_dir()`, keeping the primary configuration root directory pristine and organized.
  - Implemented the industry-standard general sequential master log file (`logs/wintil.log`) capturing all runtime events, diagnostic messages, background automation executions, and keyboard unsticking actions chronologically in formatted `[YYYY-MM-DD HH:MM:SS] [LEVEL] [module] message` format.
  - Centralized all specialized log outputs into the master logs directory:
    - `logs/wintil.log`: Master chronological sequential general log.
    - `logs/wintil_crash.log`: Human-readable crash and disaster rollback incident reports with NTSTATUS exception decoding.
    - `logs/wintil_events.jsonl`: Machine-readable structured telemetry stream with timestamps, levels, modules, messages, and metadata.
    - `logs/automations.log`: Execution output and timestamps for scheduled automation tasks.
    - `logs/keyboard_audit.log`: Diagnostic traces for keyboard modifier latching, hardware controller audits, and unstick pulses.
  - Built automatic legacy log migration (`migrate_legacy_logs()`): when Wintil initializes, any existing loose log files in `~/.config/wintil/` are automatically migrated and appended into `logs/` without data loss.
  - Updated `clear_logs()` and `get_log_summary()` in `src/sentry/mod.rs` to comprehensively manage and report aggregate log storage across the entire `logs/` directory.
- **Recursive Automation Folder Deletion (`src/automations/mod.rs`)**:
  - Fixed an issue where deleting an automation via the GUI only removed its `automation.toml` manifest, leaving the parent directory and script files (e.g. `custom-automation/task.ps1`) orphaned on disk.
  - Updated `delete_automation()` to perform safe, recursive directory purging (`fs::remove_dir_all(&auto.dir_path)`), ensuring all scripts and assets belonging to a deleted automation are completely removed.
  - Added unit test `test_delete_automation_removes_directory` verifying full folder purge on deletion.
  - Cleaned up the existing orphaned `~/.config/wintil/automations/custom-automation/` folder from disk.
- **Task Scheduler XML Registration & Battery/Idle Termination Prevention (`src/startup.rs`)**:
  - Resolved sudden disappearance/termination of Wintil when running on battery or resuming from idle (`0x8007042B: ERROR_PROCESS_ABORTED`).
  - Root Cause: Default command-line task creation (`schtasks.exe /create /sc onlogon`) applies restrictive Windows power defaults: `<StopIfGoingOnBatteries>true</StopIfGoingOnBatteries>`, `<DisallowStartIfOnBatteries>true</DisallowStartIfOnBatteries>`, and `<StopOnIdleEnd>true</StopOnIdleEnd>`, causing Windows Task Scheduler to forcefully kill the entire process tree via kernel `TerminateProcess` when unplugging laptop power or resuming from idle.
  - Fix: Switched `create_or_update_task()` to write and import a declarative Task Scheduler XML definition with `<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>`, `<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>`, `<StopOnIdleEnd>false</StopOnIdleEnd>`, and `<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>`.
- **Border Stroke Overlay Standby State & Dead Window Pruning (`src/flyout/mod.rs`, `src/main.rs`, `src/pin/stroke.rs`)**:
  - Fixed an issue where the Border Stroke Overlay reported `● Tracking Active` (green) on both the Native Services dashboard and Pinned Windows options subview even when zero windows were pinned.
  - Options subview now accurately reflects `○ Standby` (amber/standby) when stroke tracking is enabled in configuration but no windows are actively pinned, and `● Tracking Active` (green) only when one or more windows are pinned.
  - Services dashboard now marks Border Stroke Overlay as `○ Standby` with details `"Standby (0 pinned windows)"` when idle, and `● Active` with `"Tracking N pinned windows (Xpx)"` when actively tracking.
  - Implemented `StrokeManager::prune_dead()` and `StrokeManager::list_overlay_hwnds()` in `src/pin/stroke.rs` to clean up orphaned stroke overlay windows and release the `SetWinEventHook` listener when all pinned windows close, preventing dormant background tracking hooks.
  - Added unit test `test_stroke_manager_standby_lifecycle` verifying idle standby overlay count and hook release behavior.
- **Zero-Lag UI & Subprocess Spawning Elimination on Mouse Hover (`src/flyout/mod.rs`, `src/startup.rs`, `src/sentry/mod.rs`)**:
  - Eliminated severe input lag and mouse hover stutter across the flyout UI.
  - Root Cause: `WM_MOUSEMOVE` was calling `app_get_flyout_snapshot()` on every mouse pixel movement event to check `show_tooltips`. This synchronously executed `schtasks.exe /query /tn Wintil /xml` (taking 40-60ms per call), scanned the `logs/` directory, and read `wintil_events.jsonl` from disk, flooding the Win32 message pump with dozens of child processes per second during mouse sweeps.
  - Fixes:
    - Cached `show_tooltips` directly in `FlyoutState`, eliminating all `app_get_flyout_snapshot()` calls during `WM_MOUSEMOVE` and reducing message handling time from ~50ms to <1 microsecond.
    - Added an in-memory 5-second TTL cache for `query_task_status()` in `src/startup.rs`, eliminating repetitive `schtasks.exe` process spawns during repainting while invalidating immediately upon task toggles.
    - Added in-memory 3-second TTL caching for `get_log_summary()` and `load_crash_history()` in `src/sentry/mod.rs`, eliminating repetitive disk I/O on hover.
  - Result: Button hover transitions and animations now render instantly (<0.2ms total frame render time) at high refresh rates (60/144Hz) with zero lag.
- **Automations Page Button Uniformity & Inner Card Framing (`src/flyout/mod.rs`)**:
  - Applied the exact inner component styling from the Settings page to the Automations options view: extended the lighter grey inner card frame down to `y: 542` (`scale.rect(16, 90, 404, 542)`).
  - Placed the `[ Create ]` (green accent) and `[ Delete ]` (danger red) action buttons neatly inside this card at the bottom (`scale.rect(28, 496, 206, 532)` and `scale.rect(214, 496, 392, 532)`), maintaining 12px side insets, 36px height, and an 8px button gap matching the Settings tab's Exit button.
  - Sized the automations list items to fit comfortably above the action buttons, preserving the `y: 550..570` bottom status line across all tabs.
- **Native Services Pure Status Log Display (`src/flyout/mod.rs`)**:
  - Removed both log action buttons (`[ Open Logs ]` and `[ Clear Logs ]`) from the Crash Sentry row on the Native Services page, transforming the row into a clean, pure status log display.
  - Sentry service row now follows the exact visual hierarchy of all other native services: Service Name on the left (`28..260`), status pill right-aligned at `x: 392`, and live telemetry details on Line 2 (`28..392`).
  - Consolidated diagnostic log management (opening the log master directory in Explorer, log purging, and circuit breaker resets) into the dedicated Crash Recovery & Telemetry card in Settings.
- **Native Services Naming & Inline Horizontal Button Alignment (`src/flyout/mod.rs`)**:
  - Retitled the primary services view to **Native Services** with subtitle *"Core Wintil background daemons and supervisors"*, cleanly distinguishing built-in system monitors from user-defined Subservices.
  - Resolved the awkward vertical button stacking in the Crash Sentry row by moving the `[ Clear Logs ]` button onto Line 1 horizontally next to the service name (`scale.rect(184, row_y, 258, row_y + 19)`).
  - Kept the button closer to the left, allowing all status indicator pills (`● Active`, `✕ Disabled`, `○ Standby`, `✕ Tripped`) across all four service rows to maintain an uninterrupted, razor-straight vertical alignment along the right card margin at `x: 392`.
  - Expanded Line 2 details text (`det_rc`) across the full width of the card (`28..392`).
- **Automations Options Page Bottom Action Bar & Header Streamlining (`src/flyout/mod.rs`)**:
  - Removed the redundant "Automations" title header from the Options Automations subview.
  - Sized the automations list card to start cleanly at `y: 90` and run down to `y: 490`, expanding vertical capacity to display up to five automation cards simultaneously.
  - Moved the `[ Create ]` (green accent) and `[ Delete ]` (danger red) buttons to the bottom of the page (`scale.rect(16, 498, 206, 542)` and `scale.rect(214, 498, 404, 542)`), creating a balanced, side-by-side action bar terminating exactly at `y: 542` for strict pane uniformity.
- **Settings Tab Streamlining & Pane Uniformity (`src/flyout/mod.rs`)**:
  - Removed redundant "Settings" header text so the tab opens directly with the Keyboard Shortcuts card starting at `y: 54`.
  - Established cross-pane geometry uniformity: all panes (Services, Options, and Settings) reach consistent bottom bounds at `y: 542` with an 8px margin before the footer.
  - Positioned the dynamic Wintil version indicator (`Wintil v0.2.8 | Minimalist Windows Companion`) at `y: 550..570` near the bottom of the component, matching the Services state monitor.
- **UAC Elevation Info Icon & Row-Wide Hover Tooltip (`src/flyout/mod.rs`)**:
  - Formatted the elevation row header to clean `Elevated ℹ` without parentheses.
  - Attached on-hover guidance across the entire row (label, status indicator, and toggle button) explaining that administrative elevation is required for window management operations on elevated windows and running elevated automations/tasks.
- **On-Hover Tooltips Preference Toggle (`src/config/mod.rs`, `src/flyout/mod.rs`, `src/main.rs`)**:
  - Added a dedicated `[ Tooltips: ON ]` / `[ Tooltips: OFF ]` toggle button in the Settings tab under System & Application.
  - Supported via `show_tooltips` in `GeneralConfig` (defaults to `true`), giving users the ability to enable or suppress floating hover guidance tooltips.
- **Refined Exit Button & Strict Cross-Pane Card Uniformity (`src/flyout/mod.rs`)**:
  - Replaced the large 58px floating Exit button with a refined 36px button placed neatly inside the lighter grey card frame (`scale.rect(28, 496, 392, 532)`).
  - Extended Card 4 (System & Application) to reach `y: 542`, establishing consistent card termination bounds across all tabs (Services, Options, and Settings).
- **Dual Log Actions & Spaced Layout in Services & Settings (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Added an `[ Open Logs ]` button (`FlyoutAction::OpenLogs`) alongside `[ Clear Logs ]` that directly opens the `$HOME/.config/wintil/logs/` directory in Windows File Explorer.
  - Sized and spaced both buttons horizontally across the available middle region (`scale.rect(160, row_y, 238, row_y + 20)` and `scale.rect(244, row_y, 322, row_y + 20)`), eliminating cramping while maintaining strict vertical alignment of all status indicator pills at `x: 392`.
  - Added reciprocal `[ Open Logs ]` and `[ Clear Logs ]` buttons to the Crash Watchdog card in Settings.
- **Pinned Windows Status Confirmation Symbol (`src/flyout/mod.rs`)**:
  - Replaced outdated `[X]` checkboxes with clean status confirmation buttons (`✓ Border Stroke: ON` / `○ Border Stroke: OFF` and `✓ Pin Badge: ON` / `○ Pin Badge: OFF`) paired with status indicators.
- **Active Services Card Overflow & Bounding (`src/flyout/mod.rs`)**:
  - Resolved bug where the last service entry ("Crash Sentry Watchdog") overflowed past the bottom of Card 1 and collided with the Subservices card.
  - Card 1 now calculates its height dynamically based on the registered daemon/service count with compact 36px row spacing and generous 8px top/bottom padding, ensuring all service entries sit cleanly inside the card frame.
  - Card 2 (Subservices) and the bottom status line are dynamically positioned below Card 1 with clean margins, maintaining the compact 420x580 window footprint.
- **Windows Subservice Terminology Standardization (`src/flyout/mod.rs`, `src/automations/mod.rs`)**:
  - Replaced all user-facing references to "subdaemon" with Windows-standard "Subservice" and "Subservice Supervisor" in the Automations Editor and Services Tab.
  - Added Serde aliases `subservice` and `is_subservice` to `AutomationFile` and `AutomationSubdaemon` for seamless backward and forward configuration compatibility.
- **Automation Edit Button Click Responsiveness (`src/flyout/mod.rs`)**:
  - Resolved the bug where clicking `[ Edit ]` on an automation item failed to open the Full-GUI Automation Editor.
  - Root cause: The row selection hit target (`row_rc`) spanned the full width of the card and was registered before the edit button, causing `WM_LBUTTONUP` to trigger row selection instead of editing.
  - Fix: Decoupled row selection target from the edit button, and updated mouse hit testing in `WM_LBUTTONUP` and `WM_MOUSEMOVE` to iterate in reverse order (`.rev()`), ensuring buttons reliably intercept clicks before their parent containers.

---

## [0.2.7] - 2026-11-14

### Added
- **Windows Startup & Elevation Management (`src/startup.rs`, `src/flyout/mod.rs`, `src/main.rs`)**:
  - Added a dedicated **Windows Startup Task** card to the Settings tab for managing Windows Task Scheduler integration.
  - Interactive autostart toggle button (`[ Enabled ]` / `[ Enable ]`) with live status pill ("● Active" / "○ Disabled") to enable or disable automatic launch on boot and logon.
  - Interactive execution privilege toggle button (`[ Highest ]` / `[ Elevate ]`) with live status pill ("● Elevated" / "○ Standard") to toggle between `HighestAvailable` and `LeastPrivilege` run levels.
  - Prompts for administrative UAC elevation via native Win32 `ShellExecuteW` (`runas`) when elevating from a standard user session.
  - Added unit test suite in [`src/startup.rs`](file:///C:/Users/ru/Desktop/projects/wintil/wintil/src/startup.rs) verifying Task Scheduler XML definition parsing.

### Fixed
- **UI In-Place Dismiss & Zero-Jump Resolution (`src/flyout/mod.rs`)**:
  - Fixed bug where clicking off the flyout UI caused the window to jump to the cursor's monitor or position before closing.
  - Root cause: `hide_flyout()` previously called `get_target_monitor_info()`, which read `GetCursorPos` and recalculated `target_x` and `target_y` at the click location.
  - Fix: `hide_flyout()` now captures the window's exact current coordinates via `GetWindowRect(state.hwnd, &mut current_rc)` in-place, closing smoothly where it stands without any horizontal movement or cross-monitor jumping.
- **Instant Crash Elimination on Dismiss (`0xc0000409` / `BorrowMutError`) (`src/flyout/mod.rs`)**:
  - Resolved fatal fast-fail termination (`0xc0000409` / `BEX64`) triggered when dismissing the flyout window.
  - Root cause: Holding `FLYOUT_STATE.borrow_mut()` across Win32 API calls (`SetWindowPos`, `ShowWindow`, `SetTimer`) caused synchronous messages (`WM_ACTIVATE`, `WM_DPICHANGED`) dispatched by Windows on the same thread to attempt a re-entrant mutable borrow.
  - Fix: Dropped all `FLYOUT_STATE` borrows prior to invoking any Win32 APIs, protected re-entrant message handlers (`WM_DPICHANGED`, `hide_flyout`, `hide_flyout_immediate`) with non-panicking `try_borrow_mut()`, and decoupled state checks in `toggle_flyout()`.
- **Multi-Monitor DPI & Box Rehydration Resolution (`src/flyout/mod.rs`)**:
  - Resolved the bug where the flyout GUI would rehydrate in a smaller, broken box form on multi-monitor or high-DPI displays.
  - Root cause: `show_flyout` previously queried `GetDpiForWindow` on a hidden window before positioning it, causing Windows to fall back to system DPI (96 dpi) and size the window at 420x580, while `paint_flyout` rendered at the target monitor's 125% or 150% DPI, clipping the UI within an undersized container.
  - Implemented `get_target_monitor_info()` using `MonitorFromPoint` and `GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI)` to resolve the true per-monitor DPI before computing window dimensions or initiating opening animations.
  - Implemented `get_flyout_geometry()` for canonical screen docking against work area margins across arbitrary monitor layouts and negative coordinate spaces, verified with a 5-case automated geometry test suite.
  - Synchronized `WM_DPICHANGED` with animation state to prevent animation frame timers from stomping newly applied DPI dimensions.
- **Keyboard Shortcuts Key Column Expansion (`src/flyout/mod.rs`)**:
  - Expanded the hotkey column width from 140px to 180px (`scale.rect(26, hk_y, 206, hk_y + 16)`), completely eliminating clipping on longer hotkeys like `"Alt + Shift + Backspace"`.
  - Compacted Settings card layout into 4 balanced cards (Shortcuts, Startup Task, Configuration File, Buffer Recovery) ensuring all controls comfortably fit within the base 580px window height.
  - Fixed duplicate click target registration on the Exit Wintil button.

---

## [0.2.6] - 2026-11-14

### Added
- **Full-GUI Task Scheduler abstraction editor (`src/flyout/mod.rs`, `src/automations/mod.rs`, `src/main.rs`)**:
  - Clicking `[ Edit ]` or `[ Create ]` overtakes the flyout interface to provide ample room for modifying and viewing task scheduling properties.
  - **Metadata card**: Displays task name, description, enabled state, and an interactive toggle for **Subdaemon Supervisor Integration** (which dynamically links the automation into the Services supervisor dashboard with watchdog monitoring).
  - **Triggers card**: Declarative event trigger abstraction layer:
    - **System Startup (`on_startup`)**: Toggles execution on boot and daemon initialization.
    - **Sleep / Wake Resume (`on_wake`)**: Toggles execution when system resumes from sleep or hibernation.
    - **Inactivity Resume (`on_inactivity_resume`)**: Toggles execution when user input resumes after idle.
    - **Idle Threshold Stepper (`inactivity_threshold_secs`)**: Interactive `[ - 15s ]` and `[ + 15s ]` adjustment buttons with live second/minute display.
  - **Execution & Script Source card**: Displays target script, working directory, `[ Switch Type ]` interpreter cycler (`.ps1`, `.bat`, `.cmd`), `[ Open Folder ]` Explorer launcher, and `[ Edit TOML ]` Notepad launcher for direct manifest customization.
  - **Bottom action bar**: Provides `[ Save Automation ]` (green accent), `[ Delete ]` (red danger, when editing existing tasks), and `[ Cancel ]` navigation.
  - Added programmatic manifest persistence (`save_automation`) and deletion (`delete_automation`) with automatic starter script template generation and zero-downtime registry reloading.

### Changed
- **Dynamic per-line key-value tooltip layout & screen-edge pinning (`src/flyout/mod.rs`)**:
  - Eliminated the global longest-key column lock that previously forced all lines to align with the single longest key in the tooltip (which previously caused subservice tooltips to stretch excessively).
  - Each line now calculates its key and value widths independently, placing the value immediately following its own key with natural scaled spacing (`scale.scale(6)`).
  - Values automatically word-wrap with `DT_WORDBREAK` while constraining total tooltip box width so tooltips never bleed past window or screen boundaries.
  - Implemented 3-tier boundary placement: tooltips attempt placement to the right of the cursor, fallback to the left if overflowing, and flush-pin their right edge to the boundary when space is restricted.
  - Standardized structured `Key: Value` metadata across all flyout tooltips (Services, Subservices, Pinned windows, Hidden/Cloaked windows, Automations, Shortcuts, and Config file).
- **Multi-monitor & taskbar orientation awareness (`src/flyout/mod.rs`)**:
  - Added dynamic monitor detection using Win32 `MonitorFromPoint` and `GetMonitorInfoW` relative to the cursor and tray icon.
  - Detects taskbar orientation across top, bottom, left, and right screen edges via `rcWork` and `rcMonitor` delta comparisons.
  - Centers the flyout around the cursor on horizontal taskbars and docks adjacent to vertical taskbars, clamping within the active monitor's work area to prevent offscreen placement on secondary displays.
  - Dynamically switches slide animation directions (sliding down from top taskbars, sliding up from bottom taskbars).
- **Automations subview header and row controls (`src/flyout/mod.rs`)**:
  - Replaced row-level `[ Run ]` buttons with `[ Edit ]` buttons (reserving script execution strictly for the Subservices dashboard in Tab 1).
  - Added `[ Create ]` (green accent) and `[ Delete ]` (red danger) buttons directly in the `Automations (<count>)` header.
  - Added interactive row selection with blue accent framing.
- **Keyboard shortcuts spacing & key:value hover previews (`src/flyout/mod.rs`)**:
  - Expanded the gutter between the hotkey column and description column from 2px to 14px, widening the card frame to `scale.rect(16, 88, 404, 292)` to completely eliminate text truncation.
  - Added on-hover interaction targets across all keyboard shortcut rows.
  - Upgraded `draw_floating_tooltip` to support structured `key: value` pairs, rendering labels in accent blue (`rgb(96, 165, 250)`), separator colons with column alignment, and values in crisp white (`rgb(245, 245, 250)`).
  - Attached rich `Hotkey: ...`, `Action: ...`, and `Details: ...` metadata to every shortcut row.

---

## [0.2.5] - 2026-11-14

### Fixed
- **Eliminated rogue HUD badge popup when adjusting stroke thickness (`src/main.rs`)**:
  - Identified the root cause of the "Pinned" notification badge appearing whenever the user adjusted border stroke thickness: stepping thickness saved `wintil.toml`, which triggered `TIMER_CONFIG_WATCHER`, which contained an unintended call to `show_pin_hud(hwnd, true, ...)` on every config file reload.
  - Removed `show_pin_hud` from the automatic configuration watcher loop.
  - Added timestamp synchronization on internal config saves (`st.last_config_modified = mod_time`) in `app_step_stroke_thickness`, `app_toggle_stroke`, and `app_toggle_hud` to prevent redundant file-watcher reload cycles.
- **Fixed test badge placement and corner spawning (`src/pin/hud.rs`, `src/window/mod.rs`, `src/flyout/mod.rs`, `src/main.rs`)**:
  - Fixed an issue where clicking "Test Badge" in the flyout GUI spawned the badge offscreen at `(-100, 20)` in the top-left corner of the monitor. `app_test_hud` was previously passing `MAIN_HWND` (a hidden 0-sized message window), causing `GetWindowRect` to return zeros.
  - Centralized `get_visible_window_bounds` in [`src/window/mod.rs`](file:///C:/Users/ru/Desktop/projects/wintil/wintil/src/window/mod.rs) using DWM extended frame bounds.
  - Added `get_flyout_hwnd()` in [`src/flyout/mod.rs`](file:///C:/Users/ru/Desktop/projects/wintil/wintil/src/flyout/mod.rs): `app_test_hud()` now targets the active flyout window, centering the test badge directly on top of the flyout GUI.
  - Added a smart fallback in [`src/pin/hud.rs`](file:///C:/Users/ru/Desktop/projects/wintil/wintil/src/pin/hud.rs): if the target window is invalid or non-existent, the badge centers horizontally near the top of the monitor's work area instead of spawning in the corner.

---

## [0.2.4] - 2026-11-14

### Fixed
- **Dynamic process-hugging pin border overlay (`src/pin/stroke.rs`)**:
  - Fixed an issue where the pin border overlay spawned up to 8px away from the pinned window, leaving ~6px of transparent gap when using a 2px stroke.
  - Replaced legacy `GetWindowRect` calls (which include the invisible 7-8px drop shadow and resize margins of Windows 10/11 windows) with `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` to capture the exact visible window perimeter.
  - Implemented dynamic process hugging (`get_overlay_geometry`):
    - Non-maximized windows: bounds positioned at `visible.left - thickness, visible.top - thickness` with dimensions `visible.width + 2*thickness, visible.height + 2*thickness`. The inner hollow cut-out matches the visible window perimeter with exact 0px tolerance, completely eliminating floating gaps.
    - Maximized windows: automatically frames the inner boundary of the work area so borders do not bleed offscreen or onto adjacent monitors.
    - Real-time updates: whenever stroke thickness is adjusted (`+` / `-`) or reloaded, overlays immediately recalculate their bounding rectangles and hollow cut-outs to hug the window at the new thickness.

---

## [0.2.3] - 2026-11-14

### Added
- **Unified per-monitor conditional display scaling across all rendering subsystems (`src/pin/hud.rs`, `src/pin/stroke.rs`, `src/flyout/mod.rs`)**:
  - Brought all on-screen UI elements into compliance with the Per-Monitor v2 DPI scaling standard:
    - **Pin HUD Notification Badge (`src/pin/hud.rs`)**: Dynamically queries target window DPI via `GetDpiForWindow` to scale badge dimensions (`160x36` scaled), font point size (`18pt` scaled), corner curvature (`8px` scaled), font stroke dilation, and contextual vertical placement offset (`16px` scaled) so badges appear physically consistent and razor-sharp across 100%, 125%, 150%, and 200% displays.
    - **Custom Image Assets (`src/pin/hud.rs`)**: External HUD artwork (`assets/pinned.png`, `assets/unpinned.png`) automatically scales proportionally via GDI+ bicubic interpolation based on the monitor's scale factor.
    - **Pin Border Stroke Overlay (`src/pin/stroke.rs`)**: Overlay border thickness dynamically scales per monitor (`get_scaled_thickness`), preserving visual proportions across standard and high-DPI displays while ensuring a crisp minimum 1px boundary.
    - **Flyout Screen Positioning Margin (`src/flyout/mod.rs`)**: Scaled the 12px work area padding margin from the taskbar (`scale.scale(12)`), preventing flyout crowding on high-density displays.

---

## [0.2.2] - 2026-11-14

### Added
- **Consolidated rounded menu buttons, cards, and UI geometry (`src/flyout/mod.rs`)**:
  - Replaced legacy boxy, rectangular internal controls with modern rounded geometry matching Windows 11 Fluent Design and the flyout's own rounded window frame (`DWMWCP_ROUND`).
  - Implemented custom Win32 GDI rounded box rendering (`draw_rounded_box`) using `RoundRect`, `CreatePen`, and `CreateSolidBrush` with automatic GDI handle management.
  - Standardized corner radii across all flyout elements:
    - Tab bar header navigation buttons (`Services`, `Options`, `Settings`): 8px radius.
    - Options submenu pill selectors (`Pinned`, `Hidden`, `Automations`): 10px radius.
    - Outer card frames across all views (Services, Subservices, Pinned list, Border stroke controls, Notification badge, Cloaked list, Auto-hide rules, Automations, Shortcuts, Config file, Keyboard recovery): 10px radius.
    - Interactive action buttons (`Unpin All`, `Unpin`, `Enable Border Stroke`, `+/-`, `Show Badge`, `Test Badge`, `Restore Last`, `Restore All`, `Clear List`, `Restore`, `Run Fix` / `Run`, `Open Config`, `Reload Config`, `Unstick Keys`, `Exit Wintil`): 8px radius.
    - Floating hover tooltips: 6px radius.
  - All corner radii dynamically scale with per-monitor DPI (`DpiScale`), ensuring rounded contours remain sharp, smooth, and distortion-free on high-DPI and 1920x1200 displays.

---

## [0.2.1] - 2026-11-14

### Added
- **Smooth flyout entrance & exit animations (`src/flyout/mod.rs`)**:
  - Replaced jarring instant pop-in with a hardware-composited Windows 11-style entrance animation using `WS_EX_LAYERED`, `SetLayeredWindowAttributes`, and `DWMWCP_ROUND`.
  - The flyout floats up 18px while smoothly blooming from 10% to 100% opacity via cubic ease-out over 140ms at ~100 FPS without tearing, flicker, or DWM corner clipping.
  - Dismissing the flyout (via click outside, Escape, or tray toggle) smoothly fades out over 90ms with subtle downward drift.
- **Tray toggle dismissal debounce to eliminate rehydration (`src/main.rs`, `src/flyout/mod.rs`)**:
  - Fixed an issue where clicking the system tray icon while the flyout was open caused the window to deactivate and immediately reopen ("rehydrate").
  - Bound `WM_LBUTTONDOWN` and deactivation tracking with a 350ms dismissal cooldown: clicking the tray icon when the flyout is open closes it cleanly and keeps it closed.
- **Hover tooltip delay (`src/flyout/mod.rs`)**:
  - Implemented a 450 ms hover timer (`TIMER_TOOLTIP_HOVER = 991`, `TOOLTIP_HOVER_DELAY_MS = 450`). Tooltips only reveal when the cursor rests steadily over an element, eliminating popup flicker during fast mouse traversal.
- **Declarative hotkey and GUI-driven auto-hide lifecycle (`src/main.rs`, `src/window/mod.rs`, `src/flyout/mod.rs`)**:
  - `Alt + Shift + H` dynamically captures the exact window under the mouse cursor at invocation time using `WindowFromPoint` and `GetAncestor(GA_ROOT)` (falling back to foreground window), preventing hiding unintended windows when switching apps.
  - Automatically spins up the background auto-hide service upon hiding if it was not already active.
  - When all hidden windows are restored or cleared from the GUI (`Restore`, `Restore All`, `Clear List`), the auto-hide service automatically stops and kills itself if no other rules are active.
- **Per-monitor dynamic DPI scaling (`src/flyout/mod.rs`)**:
  - Added `DpiScale` calculation via `GetDpiForWindow` and `GetDpiForSystem` across the flyout and HUD rendering paths, ensuring crisp typography, proper padding, and sharp borders across 1920x1200 (16:10) and high-DPI displays without blurriness.
- **Global emergency keyboard unstick hotkey (`src/hotkeys/mod.rs`, `src/config/mod.rs`)**:
  - Bound `Alt + Shift + Backspace` (`HotkeyAction::UnstickKeys = 110`) to immediately purge stuck keys, reset modifier tables, and shatter typematic repeat loops system-wide without opening the GUI.
- **Subservices view & dedicated card layout (`src/flyout/mod.rs`)**:
  - Renamed the secondary section in Tab 1 to **Subservices**, expanding available vertical space (~268px) to comfortably show multiple registered subdaemons, real-time status badges, timestamps, and one-click action triggers.
- **Settings tab configuration watcher reporting (`src/flyout/mod.rs`)**:
  - Moved live configuration watcher status from Tab 1 into the Settings tab under the configuration file path, eliminating UI bloat while retaining visibility into automatic config reloads.

### Fixed
- **Eliminated rogue input injection and phantom spacebar on inactivity resume (`src/main.rs`, `src/hotkeys/audit.rs`)**:
  - Identified the root cause of random inputs (e.g. YouTube pausing or clicking when moving the mouse after brief pauses): an overzealous idle check was previously triggering an unstick sweep on resume that dispatched `KEYEVENTF_KEYUP` for `Space` (`0x20`), `Tab`, `Return`, `Escape`, and `VK_F24`.
  - Completely removed synthetic input dispatch from the idle/inactivity resume path. Moving the mouse after being idle now injects zero keystrokes.
  - Restored `clean_release_held_modifiers()` to its pure, safe design: it inspects `GetAsyncKeyState` and releases *only* modifier keys (`Alt`, `Shift`, `Ctrl`, `Win`) that are physically held down when a window disappears. If no modifiers are held, it sends zero events.
  - Removed Space (`0x20`), Enter (`0x0D`), Tab (`0x09`), and Escape (`0x1B`) from unstick scans to guarantee that unsticking never causes stray button clicks, form submissions, or media pauses in focused applications.
  - Reset inactivity detection threshold to 60 seconds (1 minute), preventing brief reading or video pauses from triggering idle transitions.
- **ASUS Zephyrus (GA402) keyboard wake and repeating key loop resolution (`automations/asus-g14-keyboard-wake/fix.ps1`, `src/hotkeys/audit.rs`, `src/main.rs`)**:
  - Identified root cause of keys getting stuck or repeating after inactivity: Windows modern power management aggressively putting the internal USB Root Hub 3.0 controller to sleep, causing the ASUS N-KEY controller firmware to latch keydown packets on wake.
  - Corrected system power plan subgroup GUID to `2a737441-1930-4402-8d77-b2bebba308a3`, ensuring USB selective suspend is disabled on both AC and Battery.
  - Set `HcDisableSelectiveSuspend = 1` in USB host controller registry and disabled selective suspend across all USB Root Hub 3.0 devices and the ASUS composite device.

### Changed
- **Flyout UI renaming and Title Capitalization (`src/flyout/mod.rs`, `src/main.rs`)**:
  - Renamed Tab 1 to **Services** (with header **Running Services** in Title Capitalization).
  - Streamlined background services to 4 core engines with intuitive names: **Auto-Hide Manager**, **Border Stroke Overlay**, **Automations Engine**, and **Crash Sentry Watchdog**.
  - Compacted the Running Services list height to allow generous vertical clearance for Subservices.
  - Removed the manual `[ Start ]` / `[ Unload ]` toggle button from the Auto-Hide Manager row in Services, expanding the details line to full width (388px) without truncated or squeezed text.
  - Centered submenu pill selectors in the Options tab by shortening labels to **Pinned**, **Hidden**, and **Automations**, giving comfortable margin padding on both sides.
  - Enforced strict **Title Capitalization** across all button labels and pill selectors throughout the entire UI (`[ Run Fix ]`, `[ Unpin All ]`, `[ Restore Last ]`, `[ Restore All ]`, `[ Clear List ]`, `[ Open Config ]`, `[ Reload Config ]`, `[ Unstick Keys ]`, `[ Test Badge ]`, `[ Exit Wintil ]`, `[ Pinned ]`, `[ Hidden ]`, `[ Automations ]`).

---

## [0.2.0] - 2026-11-14

### Added
- **Flyout GUI overhaul with 3-tab layout and default Daemons view (`src/flyout/mod.rs`)**:
  - Reorganized the popout flyout into three dedicated views:
    - **Daemons (Tab 1, default open view)**: Displays real-time operational status for all background subsystems (Auto-Hider, Pin Border Stroke, Automations Engine, Crash Sentry, and Config Watcher). Active subsystems show `[ACTIVE]` with resource stats, while inactive subsystems show `[STANDBY (Lazy-Unloaded)]` (0 threads, 0 CPU).
    - **Options (Tab 2, dynamic middle view)**: Interactive submenu pill selector at the top toggling between **Pinned Apps**, **Hidden Windows**, and **Automations**.
    - **Settings (Tab 3)**: Hotkey reference matrix, declarative config path with one-click open and live reload, modifier buffer sanitizer, and clean application exit.
- **Lazy loading subsystem architecture ("run only what needs to be run")**:
  - **Auto-Hider Daemon**: If `auto_hide_executables` is empty (default), the background thread is not spawned at all (`None`). Only starts when target executable rules exist, and automatically unloads when rules are cleared.
  - **Pin Border Stroke Manager**: The WinEvent hook (`EVENT_OBJECT_LOCATIONCHANGE`) is completely detached when 0 windows are pinned. Hooks are only attached when at least one pinned window exists and border stroke is enabled.
  - **Automations Engine**: Event-driven via Win32 `WM_POWERBROADCAST` and input idle checks; consumes zero background polling threads.
- **Hardware automations engine & subdaemon supervisor (`src/automations/mod.rs`)**:
  - Discovered and addressed the root cause of stuck arrow keys after Modern Standby sleep on ASUS Zephyrus laptops (ASUS N-KEY controller `DeviceSelectiveSuspended` power cutoff).
  - Built an automations engine that loads declarative `automation.toml` configs from `automations/` and `~/.config/wintil/automations/`. Supports triggers for `on_startup`, `on_wake` (`WM_POWERBROADCAST`), and `on_inactivity_resume` (`GetLastInputInfo`).
  - Added support for `[subdaemon]` manifests (`is_subdaemon = true`), exposing dedicated cards in the "Running Daemons" tab with one-click `[ Run Fix ]` triggers and background execution.
  - Added CLI commands `--list-automations` and `--run-automation <NAME>`.

### Fixed
- **Cleaned up input buffers and fixed mouse disruption (`src/hotkeys/audit.rs`, `src/main.rs`)**:
  - **Removed phantom keydowns**: Dropped the experimental `mask_menu_key()` function that was injecting synthetic `0xE8` keydowns into the raw input buffer. That injection was interleaving with mouse packets and colliding with other Alt-based shortcuts like PowerToys Run (`Alt + Space`) and Windows task switching (`Alt + Tab`), making mouse clicks behave erratically or register as `Alt + Click`.
  - **Replaced blanket modifier bursts**: The previous hotkey release logic was injecting 11 synthetic key-up events on every action whether keys were pressed or not. Replaced this with `clean_release_held_modifiers()`, which checks `GetAsyncKeyState` first and only releases keys that are physically held down, and only when a window is actually hidden (`HideToTray`) or closed/killed. Standard actions like maximizing or pinning do not touch the input stream at all.
  - **Removed post-hotkey watchdog timer**: Got rid of the 120 ms `TIMER_KEY_SANITIZER` background timer. Wintil no longer injects delayed synthetic input into the Windows queue behind the scenes.
- **Made window location hook idle by default (`src/pin/stroke.rs`)**:
  - `SetWinEventHook` (`EVENT_OBJECT_LOCATIONCHANGE`) was previously listening system-wide even when pin border stroke was disabled. It now only hooks when `pin_stroke.enabled` is true and at least one window is actually pinned. During normal operation with pin stroke off (the default), zero global hooks run on the system.

### Added
- **Unified compile-time versioning (`src/version.rs`)**:
  - Centralized application version and banner strings so they are pulled directly from `Cargo.toml` at compile time via `env!("CARGO_PKG_VERSION")`.
  - Unified references across the flyout footer, `--version` / `--help` CLI flags, audit monitor, crash sentry logs, and default tray tooltip.
- **Version bump and build script (`scripts/bump_version.py`)**:
  - Added a versioning script matching the workflow from the `fig` project (`python scripts/bump_version.py patch --build --test --reload`) to check consistency, bump SemVer, update docs, compile release binaries, and reload the running task in one step.
  - Added helper batch files (`scripts/bump.bat`, `scripts/build.bat`).

---

## [0.1.9] - 2026-11-13

### Fixed
- **Addressed stuck keys when hiding or closing windows (`src/hotkeys/audit.rs`)**:
  - Fixed an issue where modifier keys (`Alt`, `Shift`, `Ctrl`) would stay stuck down after using hotkeys like `Alt + Shift + H` (hide to tray) or `Alt + Q` (close). Because the focused window vanished while the user's fingers were still physically holding the shortcut, Windows dropped the subsequent `WM_KEYUP` messages. Added targeted modifier release on window destruction to keep the OS key state clean.

### Added
- **Live keyboard telemetry and audit tool (`--audit-keys`)**:
  - Added a read-only terminal monitor run via `wintil.exe --audit-keys` (or `--audit`). Displays physical hardware state (`GetAsyncKeyState`) vs thread logical state (`GetKeyState`) for `Alt`, `Shift`, `Ctrl`, and `Win` 20 times per second, flagging any software desyncs in real time without injecting fake keys.
  - Added telemetry logging to `$HOME/.config/wintil/keyboard_audit.log`.
  - Added `wintil.exe --flush-keys` CLI command and a "Flush Keys" button in the tray and flyout settings tab to easily unstick keys if another tool soft-locks them.

---

## [0.1.8] - 2026-11-13

### Changed
- **Full native rewrite from AutoHotkey to Rust**:
  - Rebuilt Wintil completely from the original AutoHotkey v2 script (`win-utils`) into a standalone native Windows binary written in Rust.
  - Replaced low-level keyboard hooks (`WH_KEYBOARD_LL`) with kernel-level `RegisterHotKey`, getting rid of the typing/gaming input latency that comes with AHK hooks.
  - Reduced memory usage to <3 MB with zero runtime dependencies.
- **New Popout Flyout GUI (`src/flyout/mod.rs`)**:
  - Replaced deep, cascading right-click tray submenus with a small Quick Settings-style popout flyout anchored in the bottom-right of the desktop.
  - Solved the issue where submenus would flip left and right across monitor edges and overlap each other.
  - Stateful tabs for **Pinned Windows**, **Hidden Windows**, and **Settings & Hotkeys**.
  - Left-clicking the tray icon toggles the flyout on demand, while right-clicking gives you a flat context menu.
- **Crash Recovery Sentry (`src/sentry/mod.rs`)**:
  - Added an out-of-process watchdog process (`wintil.exe --sentry <PID>`) that sleeps in the background at zero CPU.
  - Maintains an atomic state journal (`~/.config/wintil/wintil_state.journal`) of all cloaked and pinned windows.
  - If Wintil ever crashes or gets force-killed, the sentry wakes up instantly and unhides all windows and restores window titles so no windows are permanently lost.
- **Canonical config location (`src/config/mod.rs`)**:
  - Moved configuration to `$HOME/.config/wintil/wintil.toml` with automated directory and file creation on first run.
  - Background file watcher reloads configuration automatically whenever `wintil.toml` is saved.
- **Cleaned baseline configuration**:
  - Removed personal executable entries (like DDNS clients) from defaults; `auto_hide_executables` now defaults to an empty list `[]`.

---

## [0.1.7] - 2026-11-13

### Fixed
- **Fixed crash when opening tray menus (`src/main.rs`)**:
  - Fixed a `RefCell` re-entrancy panic (`AlreadyBorrowedError`) that happened when the config watcher timer ticked while the right-click tray menu was open.
  - Switched timers to `try_borrow_mut()` and dropped borrows before calling modal Win32 menu functions.

---

## [0.1.6] - 2026-11-13

### Added
- **Declarative pin badge styling**:
  - Added `[pin_hud]` section in `wintil.toml` to customize badge text, background color, text color, outline stroke, and opacity.
  - Supports hex colors (`#RRGGBB`) and color names.
- **Live config watcher**:
  - Added automatic file modification watcher that reloads hotkeys and styles whenever `wintil.toml` is saved, showing a brief preview badge on reload.

---

## [0.1.5] - 2026-11-13

### Fixed
- **Fixed window dragging deadlock with pin stroke (`src/pin/stroke.rs`)**:
  - Removed multi-threaded DwmFlush loop that locked up Wintil when dragging pinned windows.
  - Switched to an event-based overlay using GDI regions so mouse clicks pass straight through to the pinned window.

---

## [0.1.4] - 2026-11-13

### Fixed
- **Fixed invisible pin/unpin text on HUD badge (`src/pin/hud.rs`)**:
  - Fixed a bug where the text did not show up because the bitmap buffer was not selected into the memory DC before calling `DrawTextW`.
  - Added a clean black outline stroke around white text so it is easy to read over any background.
  - Changed background to a softer slate gray (`#383E48`) at 75% opacity.

---

## [0.1.3] - 2026-11-13

### Fixed
- **Fixed missing tray icon on startup (`src/tray/mod.rs`)**:
  - Fixed an issue where the tray icon would sometimes not show up when Wintil launched via Task Scheduler on logon.
  - Registered `TaskbarCreated` message and configured UIPI message filters so Wintil re-adds its tray icon whenever Explorer starts or restarts.
  - Added a startup retry timer that checks up to 10 seconds if Explorer was not ready yet on boot.

---

## [0.1.0] - 2026-11-12

### Added
- **Initial native Rust port**:
  - First standalone port from AutoHotkey v2 (`win-utils`).
  - Added basic window hotkeys: maximize/restore (`Alt + F`), minimize (`Alt + H`), close (`Alt + Q`), process kill (`Alt + Shift + Q`), and hide to tray (`Alt + Shift + H`).
  - Added background auto-hider daemon to tuck away designated background executables automatically.
  - Added pin always-on-top (`Alt + P`) with on-screen HUD badge and `[Pinned]` title tagging.
  - Added fail-safe shutdown handlers to unhide all cloaked windows when exiting.
