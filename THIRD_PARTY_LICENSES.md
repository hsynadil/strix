# Third-party notices

Strix bundles the following third-party components (in
`src-tauri/resources/sensors/`) to read hardware temperatures. They are shipped
unmodified.

## LibreHardwareMonitorLib

- Source: https://github.com/LibreHardwareMonitor/LibreHardwareMonitor
- License: **Mozilla Public License 2.0 (MPL-2.0)** — https://www.mozilla.org/MPL/2.0/
- Files: `LibreHardwareMonitorLib.dll`

Under MPL-2.0 the source of this component is available at the URL above. Strix
uses it, unmodified, via a small helper process (`strix-sensors.exe`, source in
[`sensor-helper/`](sensor-helper/)) that reads temperature sensors and prints JSON.
The kernel driver used to read CPU sensors (PawnIO) is provided/installed by
LibreHardwareMonitor and requires administrator rights.

## HidSharp

- Source: https://github.com/IntergatedCircuits/HidSharp
- License: Apache-2.0 / MIT (dual)
- Files: `HidSharp.dll`

## .NET Framework support assemblies

Small Microsoft polyfill assemblies (`System.Memory.dll`, `System.Buffers.dll`,
`System.Numerics.Vectors.dll`, `System.Runtime.CompilerServices.Unsafe.dll`,
`Microsoft.Bcl.HashCode.dll`) are redistributed under the MIT License.
