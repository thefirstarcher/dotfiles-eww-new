#!/usr/bin/env python3
"""
============================================================================
Volume + Audio Level Daemon for EWW
============================================================================
Monitors both volume settings and real-time audio output levels
Returns JSON with volume percent, mute status, and current audio level
"""

import subprocess
import json
import threading
import sys
import time
import struct
import math
import select


class VolumeAudioMonitor:
    def __init__(self):
        self.volume_percent = 0
        self.muted = False
        self.audio_level = 0
        self.running = True

    def get_volume_info(self):
        """Get current volume and mute status from pactl"""
        try:
            # Get volume
            vol_output = subprocess.check_output(
                ["pactl", "get-sink-volume", "@DEFAULT_SINK@"], text=True
            )
            # Extract first percentage
            for part in vol_output.split():
                if "%" in part:
                    self.volume_percent = int(part.strip("%"))
                    break

            # Get mute status
            mute_output = subprocess.check_output(
                ["pactl", "get-sink-mute", "@DEFAULT_SINK@"], text=True
            )
            self.muted = "yes" in mute_output.lower()

        except Exception as e:
            print(f"Error getting volume: {e}", file=sys.stderr)

    def monitor_volume_changes(self):
        """Monitor volume changes using pactl subscribe"""
        try:
            process = subprocess.Popen(
                ["pactl", "subscribe"], stdout=subprocess.PIPE, text=True, bufsize=1
            )

            for line in process.stdout:
                if "sink" in line.lower():
                    self.get_volume_info()
                    self.output_state()

        except Exception as e:
            print(f"Error monitoring volume: {e}", file=sys.stderr)

    def monitor_audio_level(self):
        """Monitor real-time audio levels from sink monitor"""
        try:
            # Get default sink name
            sink = subprocess.check_output(
                ["pactl", "get-default-sink"], text=True
            ).strip()

            # Start monitoring audio stream
            process = subprocess.Popen(
                [
                    "parec",
                    "--device=" + sink + ".monitor",
                    "--format=s16le",
                    "--rate=44100",
                    "--channels=1",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
            )

            chunk_size = 256  # Very small chunks for very fast updates
            last_update = time.time()
            min_interval = 0.01  # Update every 5ms for maximum responsiveness

            while self.running:
                # Read audio samples
                data = process.stdout.read(chunk_size * 2)  # 2 bytes per sample
                if not data:
                    break

                # Calculate RMS volume
                samples = struct.unpack(f"{len(data) // 2}h", data)
                rms = math.sqrt(sum(s * s for s in samples) / len(samples))

                # Convert to percentage (max value for s16le is 32768)
                # Calculate raw level
                raw_level = (rms / 32768) * 100

                # Scale by sink volume to show actual output level
                # This accounts for both app and sink volume
                # actual_level = (raw_level * self.volume_percent) / 100
                actual_level = raw_level

                # Apply amplification for visibility
                level = int(actual_level * 2.0)
                level = min(level, 100)  # Cap at 100%

                # Rate limit updates
                now = time.time()
                if now - last_update >= min_interval:
                    self.audio_level = level
                    self.output_state()
                    last_update = now

        except Exception as e:
            print(f"Error monitoring audio level: {e}", file=sys.stderr)

    def output_state(self):
        """Output current state as JSON"""
        state = {
            "percent": self.volume_percent,
            "muted": self.muted,
            "level": self.audio_level,
        }
        print(json.dumps(state), flush=True)

    def run(self):
        """Start monitoring both volume and audio levels"""
        # Get initial volume
        self.get_volume_info()
        self.output_state()

        # Start volume monitoring thread
        volume_thread = threading.Thread(
            target=self.monitor_volume_changes, daemon=True
        )
        volume_thread.start()

        # Monitor audio levels in main thread
        self.monitor_audio_level()


if __name__ == "__main__":
    monitor = VolumeAudioMonitor()
    try:
        monitor.run()
    except KeyboardInterrupt:
        monitor.running = False
        sys.exit(0)
