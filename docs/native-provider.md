# Optional native DualSense bridge

CONTROLLER can control an installed [vDS runtime](https://github.com/hurryman2212/vds) on Windows or Linux. Its virtual USB DualSense can carry native game feedback over Bluetooth. This path remains experimental and needs physical interoperability testing. macOS has no integrated native virtual-gamepad provider.

Install the upstream runtime, drivers and dependencies first. Windows uses usbip-win2, HidHide and the vDS service; Linux uses the upstream virtual host-controller module, BlueZ and vdsd. CONTROLLER does not silently install or start privileged drivers. It looks for the control tool in the standard installation folders.

On Profiles, inspect the service, select a paired Bluetooth controller, then attach it. The app binds the operation to that controller's address and model, saves local ownership before releasing its HID/audio handles, and verifies the registration through a fresh provider list. App input and effects pause. Native forwarding preserves physical reports; app calibration is applied to the separate virtual Xbox route.

Detach verifies that the registration is absent before app monitoring resumes. Timeouts, partial failures, damaged saved ownership and core restarts keep monitoring paused until recovered. Use Recover connection only after stopping the external bridge. Hardware addresses stay in the local ownership record and are redacted from provider status/errors.

The provider does not add native haptics to games that lack them or detect weapons automatically. For those games, use virtual Xbox output with input-based action rules and editable trigger profiles. Validate Steam settings and each target game separately.

Protocol reference: upstream MIT source at commit `ec531599dfdd02e14ce68efd8c0d2dcb439894a5`, including `vdsctl_common.cc` and `vdsd_common.cc`. Control replies are JSONL; attach/detach acknowledgements alone do not prove that controller registration changed.

The ownership record, health check and registration readback were added in 0.22.x. They remain unverified without an installed vDS runtime and a physical Bluetooth DualSense: attach, detach, recovery after a crash, and handoff of rumble/haptics to a real game have only been exercised against fake provider output in tests. See the [hardware test checklist](hardware-test-checklist.md).
