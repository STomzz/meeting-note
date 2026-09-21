"""音频工具单测。"""

from __future__ import annotations

import pytest

from app.core.errors import UnsupportedMediaError
from app.services.audio import ensure_supported_audio, is_wav, wav_duration_sec
from tests.conftest import make_wav_bytes


def test_is_wav():
    assert is_wav(make_wav_bytes(0.1))
    assert not is_wav(b"ID3fake")
    assert not is_wav(b"")


def test_wav_duration_sec():
    duration = wav_duration_sec(make_wav_bytes(duration_sec=2.0))
    assert duration == pytest.approx(2.0, rel=0.01)


def test_wav_duration_returns_none_for_garbage():
    assert wav_duration_sec(b"not a wav at all") is None


def test_ensure_supported_audio_accepts_wav():
    ensure_supported_audio("a.wav", "audio/wav", make_wav_bytes(0.1))


def test_ensure_supported_audio_rejects_mp3():
    with pytest.raises(UnsupportedMediaError):
        ensure_supported_audio("a.mp3", "audio/mpeg", b"ID3\x04" + b"0" * 100)
