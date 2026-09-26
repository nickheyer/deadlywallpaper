using MathNet.Numerics.IntegralTransforms;
using System;
using System.Collections.Generic;
using System.Numerics;

namespace Lively.Common.Linux.Audio
{
    /// <summary>
    /// Pure FFT + smoothing pipeline shared with the Windows NAudioVisualizerService:
    /// the first <c>samples.Length / 8</c> samples are transformed with MathNet's forward FFT,
    /// the last <see cref="VerticalSmoothness"/> spectra are averaged per bin, then neighbouring
    /// bins are averaged horizontally and the first <see cref="BinCount"/> bins are emitted.
    /// </summary>
    public sealed class SpectrumProcessor
    {
        /// <summary>Number of bins emitted per frame (matches the Windows service's maxSample).</summary>
        public const int BinCount = 128;
        /// <summary>Samples the caller must feed per frame so that samples / 8 yields exactly <see cref="BinCount"/> FFT points.</summary>
        public const int RequiredSampleCount = BinCount * 8;

        private const int VerticalSmoothness = 2;
        private const int HorizontalSmoothness = 1;

        private readonly List<Complex[]> smooth = new();

        /// <summary>Transforms one frame of mono float samples into <see cref="BinCount"/> smoothed magnitudes.</summary>
        public double[] Process(ReadOnlySpan<float> samples)
        {
            int len = samples.Length / 8;
            if (len < BinCount)
                throw new ArgumentException($"At least {RequiredSampleCount} samples are required per frame, got {samples.Length}.", nameof(samples));

            var values = new Complex[len];
            for (int i = 0; i < len; i++)
                values[i] = new Complex(samples[i], 0.0);
            Fourier.Forward(values, FourierOptions.Default);

            smooth.Add(values);
            if (smooth.Count > VerticalSmoothness)
                smooth.RemoveAt(0);

            var window = smooth.ToArray();
            var audioData = new double[BinCount];
            for (int i = 0; i < BinCount; i++)
                audioData[i] = BothSmooth(i, window);
            return audioData;
        }

        /// <summary>A frame of <see cref="BinCount"/> zero magnitudes.</summary>
        public static double[] Silence() => new double[BinCount];

        private static double BothSmooth(int i, Complex[][] window)
        {
            double value = 0;
            for (int h = Math.Max(i - HorizontalSmoothness, 0); h < Math.Min(i + HorizontalSmoothness, BinCount); h++)
                value += VSmooth(h, window);

            return value / ((HorizontalSmoothness + 1) * 2);
        }

        private static double VSmooth(int i, Complex[][] window)
        {
            double value = 0;
            for (int v = 0; v < window.Length; v++)
                value += Math.Abs(window[v][i].Magnitude);

            return value / window.Length;
        }
    }
}
