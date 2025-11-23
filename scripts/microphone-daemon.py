#!/usr/bin/env python3
"""
============================================================================
Microphone + Audio Input Level Daemon for EWW
============================================================================
Monitors both microphone volume settings and real-time audio input levels
Returns JSON with volume percent, mute status, and current input level
"""

import subprocess
import json
import threading
import sys
import time
import struct
import math


class MicrophoneAudioMonitor:
    def __init__(self):
        self.volume_percent = 0
        self.muted = False
        self.input_level = 0
        self.running = True

    def get_microphone_info(self):
        """Get current microphone volume and mute status from pactl"""
        try:
            # Get volume
            vol_output = subprocess.check_output(
                ["pactl", "get-source-volume", "@DEFAULT_SOURCE@"], text=True
            )
            # Extract first percentage
            for part in vol_output.split():
                if "%" in part:
                    self.volume_percent = int(part.strip("%"))
                    break

            # Get mute status
            mute_output = subprocess.check_output(
                ["pactl", "get-source-mute", "@DEFAULT_SOURCE@"], text=True
            )
            self.muted = "yes" in mute_output.lower()

        except Exception as e:
            print(f"Error getting microphone info: {e}", file=sys.stderr)

    def monitor_microphone_changes(self):
        """Monitor microphone changes using pactl subscribe"""
        try:
            process = subprocess.Popen(
                ["pactl", "subscribe"], stdout=subprocess.PIPE, text=True, bufsize=1
            )

            for line in process.stdout:
                if "source" in line.lower():
                    self.get_microphone_info()
                    self.output_state()

        except Exception as e:
            print(f"Error monitoring microphone: {e}", file=sys.stderr)

    def monitor_input_level(self):
        """Monitor real-time audio input levels from microphone"""
        try:
            # Get default source name
            source = subprocess.check_output(
                ["pactl", "get-default-source"], text=True
            ).strip()

            # Start monitoring audio input stream
            process = subprocess.Popen(
                [
                    "parec",
                    "--device=" + source,
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

                # Scale by source volume to show actual input level
                # actual_level = (raw_level * self.volume_percent) / 100
                actual_level = raw_level

                # Amplify for better visibility of voice input
                level = int(actual_level * 2.5)
                level = min(level, 100)  # Cap at 100%

                # Rate limit updates
                now = time.time()
                if now - last_update >= min_interval:
                    self.input_level = level
                    self.output_state()
                    last_update = now

        except Exception as e:
            print(f"Error monitoring input level: {e}", file=sys.stderr)

    def output_state(self):
        """Output current state as JSON"""
        state = {
            "percent": self.volume_percent,
            "muted": self.muted,
            "level": self.input_level,
        }
        print(json.dumps(state), flush=True)

    def run(self):
        """Start monitoring both microphone and input levels"""
        # Get initial microphone info
        self.get_microphone_info()
        self.output_state()

        # Start microphone monitoring thread
        mic_thread = threading.Thread(
            target=self.monitor_microphone_changes, daemon=True
        )
        mic_thread.start()

        # Monitor input levels in main thread
        self.monitor_input_level()


if __name__ == "__main__":
    monitor = MicrophoneAudioMonitor()
    try:
        monitor.run()
    except KeyboardInterrupt:
        monitor.running = False
        sys.exit(0)
