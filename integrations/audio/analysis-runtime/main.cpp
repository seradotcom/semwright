// SPDX-License-Identifier: MIT OR Apache-2.0
// Original fixed analysis helper. No network, device, shell or plugin interface.
#include <ebur128.h>
#include <sndfile.h>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <fcntl.h>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>
namespace {
struct SoundDelete { void operator()(SNDFILE* f) const { if (f) sf_close(f); } };
struct MeterDelete { void operator()(ebur128_state* p) const { if (p) ebur128_destroy(&p); } };
std::string milli(double value) {
    if (!std::isfinite(value) || std::abs(value) > 1000000.0) return "null";
    return std::to_string(static_cast<std::int64_t>(std::llround(value * 1000.0)));
}
void ok(int result) {
    if (result != EBUR128_SUCCESS) throw std::runtime_error("loudness analysis failed");
}
int analyze(int argc, char** argv) {
    if (argc != 4 || std::string(argv[1]) != "analyze")
        throw std::runtime_error("expected fixed analyze operation and channel layout");
    int major = 0, minor = 0, patch = 0;
    ebur128_get_version(&major, &minor, &patch);
    if (major != 1 || minor != 2 || patch != 6)
        throw std::runtime_error("libebur128 differs from pinned version 1.2.6");
    const std::string layout(argv[3]);
    if (layout != "mono" && layout != "stereo")
        throw std::runtime_error("this meter requires an explicit mono or stereo layout");
    const int fd = open(argv[2], O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) throw std::runtime_error("analysis input is unavailable");
    struct stat metadata{};
    if (fstat(fd, &metadata) || !S_ISREG(metadata.st_mode) || metadata.st_nlink != 1
        || metadata.st_size <= 0 || metadata.st_size > 536870912) {
        close(fd); throw std::runtime_error("analysis input is not a bounded regular file");
    }
    SF_INFO info{};
    std::unique_ptr<SNDFILE, SoundDelete> audio(sf_open_fd(fd, SFM_READ, &info, SF_TRUE));
    if (!audio) { close(fd); throw std::runtime_error("audio decoding failed"); }
    if (info.channels != (layout == "mono" ? 1 : 2) || info.samplerate < 8000
        || info.samplerate > 192000 || info.frames <= 0 || info.frames > 57600000)
        throw std::runtime_error("analysis media shape exceeds its declared contract");
    const auto encoding = info.format & SF_FORMAT_TYPEMASK;
    if (encoding != SF_FORMAT_WAV && encoding != SF_FORMAT_WAVEX && encoding != SF_FORMAT_FLAC)
        throw std::runtime_error("only bounded WAV and FLAC inputs are supported");
    std::unique_ptr<ebur128_state, MeterDelete> meter(ebur128_init(
        static_cast<unsigned>(info.channels), static_cast<unsigned long>(info.samplerate),
        EBUR128_MODE_I | EBUR128_MODE_S | EBUR128_MODE_LRA | EBUR128_MODE_TRUE_PEAK));
    if (!meter) throw std::runtime_error("meter allocation failed");
    ok(ebur128_set_channel(meter.get(), 0, EBUR128_LEFT));
    if (info.channels == 2) ok(ebur128_set_channel(meter.get(), 1, EBUR128_RIGHT));
    std::vector<double> samples(static_cast<std::size_t>(info.channels) * 4096);
    std::uint64_t frames = 0, nonfinite = 0;
    double sample_peak = 0;
    while (frames < static_cast<std::uint64_t>(info.frames)) {
        const auto expected = static_cast<sf_count_t>(std::min<std::uint64_t>(4096,
            static_cast<std::uint64_t>(info.frames) - frames));
        const auto count = sf_readf_double(audio.get(), samples.data(), expected);
        if (count != expected) throw std::runtime_error("truncated audio during metering");
        for (sf_count_t i = 0; i < count * info.channels; ++i) {
            if (!std::isfinite(samples[i])) { ++nonfinite; samples[i] = 0; }
            else sample_peak = std::max(sample_peak, std::abs(samples[i]));
        }
        ok(ebur128_add_frames_double(meter.get(), samples.data(), static_cast<std::size_t>(count)));
        frames += static_cast<std::uint64_t>(count);
    }
    if (sf_error(audio.get()) != SF_ERR_NO_ERROR) throw std::runtime_error("decoder reported a read error");
    double integrated = -INFINITY, momentary = -INFINITY, short_term = -INFINITY;
    double range = -INFINITY, true_peak = 0;
    if (frames * 1000 >= static_cast<std::uint64_t>(info.samplerate) * 400) {
        ok(ebur128_loudness_global(meter.get(), &integrated));
        ok(ebur128_loudness_momentary(meter.get(), &momentary));
    }
    if (frames >= static_cast<std::uint64_t>(info.samplerate) * 3) {
        ok(ebur128_loudness_shortterm(meter.get(), &short_term));
        ok(ebur128_loudness_range(meter.get(), &range));
    }
    for (int channel = 0; channel < info.channels; ++channel) {
        double channel_peak = 0;
        ok(ebur128_true_peak(meter.get(), static_cast<unsigned>(channel), &channel_peak));
        true_peak = std::max(true_peak, channel_peak);
    }
    if (nonfinite) integrated = momentary = short_term = range = -INFINITY;
    const auto peak_db = true_peak > 0 && !nonfinite ? 20 * std::log10(true_peak) : -INFINITY;
    std::cout << "{\"schema_version\":1,\"method\":\"libebur128\",\"version\":\"1.2.6\","
              << "\"frames\":" << frames << ",\"sample_rate\":" << info.samplerate
              << ",\"channels\":" << info.channels << ",\"layout\":\"" << layout << "\","
              << "\"nonfinite_samples\":" << nonfinite << ",\"integrated_lufs_milli\":" << milli(integrated)
              << ",\"momentary_lufs_milli\":" << milli(momentary)
              << ",\"short_term_lufs_milli\":" << milli(short_term)
              << ",\"loudness_range_milli\":" << milli(range)
              << ",\"true_peak_millidbtp\":" << milli(peak_db)
              << ",\"sample_peak_millidbfs\":" << milli(sample_peak > 0 ? 20 * std::log10(sample_peak) : -INFINITY);
    std::cout << ",\"true_peak_oversample\":"
              << (info.samplerate < 96000 ? 4 : info.samplerate < 192000 ? 2 : 1)
              << ",\"momentary_window_ms\":400,\"short_term_window_ms\":3000,"
              << "\"unknown_reason\":";
    if (nonfinite) std::cout << "\"nonfinite_input\"";
    else if (sample_peak == 0) std::cout << "\"silence\"";
    else if (frames < static_cast<std::uint64_t>(info.samplerate) * 3)
        std::cout << "\"one_or_more_windows_have_insufficient_frames\"";
    else std::cout << "null";
    std::cout << "}\n";
    return 0;
}
}
int main(int argc, char** argv) {
    try { return analyze(argc, argv); }
    catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
