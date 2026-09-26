using Lively.Common.Linux.Audio;
using System;
using System.Linq;
using Xunit;

namespace Lively.Common.Linux.Feeds.Tests
{
    public class SpectrumProcessorTests
    {
        private static float[] Sine(double frequencyHz, double amplitude, int sampleRate = 44100, int count = SpectrumProcessor.RequiredSampleCount)
        {
            var samples = new float[count];
            for (int i = 0; i < count; i++)
                samples[i] = (float)(amplitude * Math.Sin(2 * Math.PI * frequencyHz * i / sampleRate));
            return samples;
        }

        [Fact]
        public void SineProduces128FiniteBinsWithEnergy()
        {
            var processor = new SpectrumProcessor();
            var bins = processor.Process(Sine(440, 0.5));

            Assert.Equal(SpectrumProcessor.BinCount, bins.Length);
            Assert.All(bins, b => Assert.True(double.IsFinite(b) && b >= 0, $"bin {b} is not a finite non-negative magnitude"));
            Assert.True(bins.Max() > 0.05, $"expected audible energy, max bin was {bins.Max()}");
            Assert.True(bins.Max() < 10, $"expected a bounded magnitude for a half-scale sine, max bin was {bins.Max()}");
        }

        [Fact]
        public void SilenceProducesAllZeros()
        {
            var processor = new SpectrumProcessor();
            var bins = processor.Process(new float[SpectrumProcessor.RequiredSampleCount]);

            Assert.Equal(SpectrumProcessor.BinCount, bins.Length);
            Assert.All(bins, b => Assert.Equal(0.0, b));
        }

        [Fact]
        public void EnergyLandsInTheBinOfTheTone()
        {
            // 128 FFT points at 44100 Hz give 344.5 Hz per bin; a 3445 Hz tone peaks in bin 10 (horizontal smoothing pulls it to 10..11).
            var processor = new SpectrumProcessor();
            var bins = processor.Process(Sine(3445, 0.8));

            var peak = Array.IndexOf(bins, bins.Max());
            Assert.InRange(peak, 9, 11);
        }

        [Fact]
        public void VerticalSmoothingAveragesWithPreviousFrame()
        {
            var processor = new SpectrumProcessor();
            var loud = processor.Process(Sine(3445, 0.8));
            var afterSilence = processor.Process(new float[SpectrumProcessor.RequiredSampleCount]);

            Assert.True(afterSilence.Max() > 0, "the previous frame should still contribute through vertical smoothing");
            Assert.True(afterSilence.Max() < loud.Max(), "smoothed silence must be quieter than the loud frame");
            var silentAgain = processor.Process(new float[SpectrumProcessor.RequiredSampleCount]);
            Assert.All(silentAgain, b => Assert.Equal(0.0, b));
        }

        [Fact]
        public void RejectsFramesThatAreTooShort()
        {
            var processor = new SpectrumProcessor();
            Assert.Throws<ArgumentException>(() => processor.Process(new float[SpectrumProcessor.RequiredSampleCount - 8]));
        }

        [Fact]
        public void SilenceHelperHas128Zeros()
        {
            var silence = SpectrumProcessor.Silence();
            Assert.Equal(SpectrumProcessor.BinCount, silence.Length);
            Assert.All(silence, b => Assert.Equal(0.0, b));
        }
    }
}
