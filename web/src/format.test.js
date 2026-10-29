import { describe, expect, it } from "vitest";
import { episodeLabel, fmtBytes, fmtDuration, fmtInterval, fmtRelative, fmtTime } from "./format.js";

describe("fmtInterval", () => {
  it("uses hours under a day or when not a whole number of days", () => {
    expect(fmtInterval(3600)).toBe("1 hour");
    expect(fmtInterval(12 * 3600)).toBe("12 hours");
    expect(fmtInterval(36 * 3600)).toBe("36 hours");
  });

  it("uses days for whole days", () => {
    expect(fmtInterval(86400)).toBe("1 day");
    expect(fmtInterval(7 * 86400)).toBe("7 days");
  });
});

describe("fmtRelative", () => {
  const now = 1_000_000;

  it("describes the past", () => {
    expect(fmtRelative(now - 10, now)).toBe("just now");
    expect(fmtRelative(now - 5 * 60, now)).toBe("5m ago");
    expect(fmtRelative(now - 3 * 3600, now)).toBe("3h ago");
    expect(fmtRelative(now - 2 * 86400, now)).toBe("2d ago");
  });

  it("describes the future", () => {
    expect(fmtRelative(now + 30, now)).toBe("in <1m");
    expect(fmtRelative(now + 90 * 60, now)).toBe("in 1h");
    expect(fmtRelative(now + 6 * 86400, now)).toBe("in 6d");
  });

  it("shows a dash for a task that has never run", () => {
    expect(fmtRelative(null, now)).toBe("–");
  });
});

describe("fmtDuration", () => {
  it("scales from ms to minutes", () => {
    expect(fmtDuration(850)).toBe("850ms");
    expect(fmtDuration(4_200)).toBe("4.2s");
    expect(fmtDuration(185_000)).toBe("3m 05s");
    expect(fmtDuration(null)).toBe("–");
  });
});

describe("fmtTime", () => {
  it("formats minutes, zero-padded seconds, and milliseconds", () => {
    expect(fmtTime(0)).toBe("0:00.000");
    expect(fmtTime(83_456)).toBe("1:23.456");
    expect(fmtTime(3_600_000)).toBe("60:00.000");
  });

  it("keeps a leading sign for negative offsets", () => {
    expect(fmtTime(-1_500)).toBe("-0:01.500");
  });

  it("shows a dash when there's no value", () => {
    expect(fmtTime(null)).toBe("–");
    expect(fmtTime(undefined)).toBe("–");
  });
});

describe("episodeLabel", () => {
  it("labels an episode as Show · SxxEyy", () => {
    expect(episodeLabel({ show: "Breaking Bad", season: 1, episode: 7 })).toBe(
      "Breaking Bad · S01E07"
    );
  });

  it("omits SxxEyy when season or episode is unknown", () => {
    expect(episodeLabel({ show: "Breaking Bad", season: null, episode: 7 })).toBe("Breaking Bad");
  });

  it("labels a movie as Title (Year), or just Title without one", () => {
    expect(episodeLabel({ title: "Akira", year: 1988 })).toBe("Akira (1988)");
    expect(episodeLabel({ title: "Akira" })).toBe("Akira");
  });
});

describe("fmtBytes", () => {
  it("leaves sub-KB sizes in bytes", () => {
    expect(fmtBytes(0)).toBe("0 B");
    expect(fmtBytes(1023)).toBe("1023 B");
  });

  it("uses binary units, two decimals under 10, one decimal above", () => {
    expect(fmtBytes(1024)).toBe("1.00 KB");
    expect(fmtBytes(5.5 * 1024 ** 3)).toBe("5.50 GB");
    expect(fmtBytes(24 * 1024 ** 3)).toBe("24.0 GB");
  });

  it("tops out at TB instead of running off the unit list", () => {
    expect(fmtBytes(2048 * 1024 ** 4)).toBe("2048.0 TB");
  });

  it("shows a dash when there's no value", () => {
    expect(fmtBytes(null)).toBe("–");
  });
});
