// strix-sensors — a tiny helper that reads hardware temperatures via
// LibreHardwareMonitorLib and prints them as JSON, then exits.
//
// Strix (the Rust/Tauri app) shells out to this when running elevated, so the
// LibreHardwareMonitor kernel driver can read real CPU / GPU / board sensors.
// Written to be C# 5 compatible so it builds with the .NET Framework compiler.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;
using LibreHardwareMonitor.Hardware;

namespace StrixSensors
{
    // Visitor that refreshes every hardware node (and its sub-hardware).
    class UpdateVisitor : IVisitor
    {
        public void VisitComputer(IComputer computer) { computer.Traverse(this); }
        public void VisitHardware(IHardware hardware)
        {
            hardware.Update();
            foreach (IHardware sub in hardware.SubHardware) sub.Accept(this);
        }
        public void VisitSensor(ISensor sensor) { }
        public void VisitParameter(IParameter parameter) { }
    }

    class Program
    {
        static string Esc(string s)
        {
            StringBuilder b = new StringBuilder();
            foreach (char c in s)
            {
                if (c == '"') b.Append("\\\"");
                else if (c == '\\') b.Append("\\\\");
                else if (c == '\n') b.Append("\\n");
                else if (c == '\r') b.Append("\\r");
                else if (c == '\t') b.Append("\\t");
                else if (c < 0x20) b.Append(' ');
                else b.Append(c);
            }
            return b.ToString();
        }

        static void Collect(IHardware hw, List<string> outp)
        {
            foreach (ISensor s in hw.Sensors)
            {
                string kind;
                if (s.SensorType == SensorType.Temperature)
                {
                    // "Distance to TjMax" is a delta below the throttle point, not
                    // a real temperature — skip it.
                    if (s.Name.IndexOf("Distance to TjMax", StringComparison.OrdinalIgnoreCase) >= 0)
                        continue;
                    kind = "temp";
                }
                else if (s.SensorType == SensorType.Fan)
                {
                    kind = "fan";
                }
                else continue;

                if (!s.Value.HasValue) continue;
                double v = (double)s.Value.Value;
                if (kind == "temp") { if (v < -50.0 || v > 200.0) continue; }
                else { if (v < 0.0 || v > 100000.0) continue; }

                string label = hw.Name + " - " + s.Name;
                outp.Add(
                    "{\"label\":\"" + Esc(label) + "\",\"value\":" +
                    v.ToString("0.0", CultureInfo.InvariantCulture) +
                    ",\"kind\":\"" + kind + "\"}");
            }
            foreach (IHardware sub in hw.SubHardware) Collect(sub, outp);
        }

        static int Main(string[] args)
        {
            Computer computer = new Computer
            {
                IsCpuEnabled = true,
                IsGpuEnabled = true,
                IsMotherboardEnabled = true,
                IsControllerEnabled = true,
            };
            try
            {
                // Emit UTF-8 so the Rust side reads labels cleanly.
                Console.OutputEncoding = new UTF8Encoding(false);

                computer.Open();
                computer.Accept(new UpdateVisitor());

                List<string> items = new List<string>();
                foreach (IHardware hw in computer.Hardware) Collect(hw, items);

                Console.WriteLine("[" + string.Join(",", items.ToArray()) + "]");
                computer.Close();
                return 0;
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine(ex.Message);
                Console.WriteLine("[]");
                return 1;
            }
        }
    }
}
