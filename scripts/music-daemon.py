#!/usr/bin/env python3
# ============================================================================
# Music Player Daemon for EWW Dashboard
# ============================================================================
# Monitors MPRIS players via playerctl and allows switching between them
# Uses Event-based updates via SIGUSR1 for instant response

import subprocess
import json
import time
import os
import sys
import signal
import threading

# File paths
PID_FILE = "/tmp/eww-music-daemon.pid"
SWITCH_FILE = "/tmp/eww-music-player-switch"

class MusicDaemon:
    def __init__(self):
        self.active_player = None
        # Event to wake up the main loop
        self.update_event = threading.Event()

        # Write PID to file so control script can signal us specifically
        try:
            with open(PID_FILE, 'w') as f:
                f.write(str(os.getpid()))
        except Exception as e:
            print(f"Error writing PID file: {e}", file=sys.stderr)

        # Setup signal handler for instant updates (SIGUSR1)
        # This prevents the "User defined signal 1" crash
        signal.signal(signal.SIGUSR1, self.handle_signal)

    def cleanup(self):
        """Remove PID file on exit"""
        if os.path.exists(PID_FILE):
            try:
                os.remove(PID_FILE)
            except:
                pass

    def handle_signal(self, signum, frame):
        """Handle signal to trigger immediate update"""
        # wake up the loop immediately
        self.update_event.set()

    def get_players(self):
        """Get list of available MPRIS players"""
        try:
            result = subprocess.run(
                ['playerctl', '-l'],
                capture_output=True,
                text=True,
                timeout=2
            )
            players = result.stdout.strip().split('\n') if result.stdout.strip() else []
            # Filter empty strings
            return [p for p in players if p]
        except Exception:
            return []

    def get_metadata(self, player):
        """Get metadata for a specific player"""
        try:
            # Use a custom format to get everything in one go
            result = subprocess.run(
                ['playerctl', '-p', player, 'metadata', '--format',
                 '{{title}}|||{{artist}}|||{{mpris:artUrl}}'],
                capture_output=True,
                text=True,
                timeout=1
            )

            if result.returncode != 0:
                return None

            parts = result.stdout.strip().split('|||')
            art_url = parts[2] if len(parts) > 2 else ""

            # Strip file:// prefix and validate
            if art_url.startswith('file://'):
                file_path = art_url.replace('file://', '')
                if os.path.exists(file_path):
                    art_url = file_path
                else:
                    art_url = ""

            return {
                'title': parts[0] if len(parts) > 0 else "",
                'artist': parts[1] if len(parts) > 1 else "",
                'art_url': art_url
            }
        except Exception:
            return None

    def get_playback_status(self, player):
        try:
            result = subprocess.run(
                ['playerctl', '-p', player, 'status'],
                capture_output=True,
                text=True,
                timeout=1
            )
            return result.stdout.strip() == "Playing"
        except Exception:
            return False

    def get_position(self, player):
        try:
            pos_result = subprocess.run(
                ['playerctl', '-p', player, 'position'],
                capture_output=True,
                text=True,
                timeout=1
            )
            dur_result = subprocess.run(
                ['playerctl', '-p', player, 'metadata', 'mpris:length'],
                capture_output=True,
                text=True,
                timeout=1
            )

            # Parse position
            try:
                pos = float(pos_result.stdout.strip())
            except ValueError:
                pos = 0

            # Parse duration
            try:
                dur_micros = int(dur_result.stdout.strip())
                dur = dur_micros / 1000000
            except ValueError:
                dur = 0

            return int((pos / dur) * 100) if dur > 0 else 0
        except Exception:
            return 0

    def check_switch_request(self):
        """Check if user requested to switch player"""
        if os.path.exists(SWITCH_FILE):
            try:
                with open(SWITCH_FILE, 'r') as f:
                    new_player = f.read().strip()
                os.remove(SWITCH_FILE)
                return new_player
            except Exception:
                pass
        return None

    def get_state(self):
        available_players = self.get_players()

        # Check for manual switch
        switch_to = self.check_switch_request()
        if switch_to and switch_to in available_players:
            self.active_player = switch_to

        # Auto-select first player if none active or active lost
        if not self.active_player or self.active_player not in available_players:
            self.active_player = available_players[0] if available_players else None

        state = {
            "active_player": self.active_player or "No Player",
            "available_players": available_players,
            "title": "No Media",
            "artist": "",
            "art_url": "",
            "playing": False,
            "position_percent": 0
        }

        if self.active_player:
            metadata = self.get_metadata(self.active_player)
            if metadata:
                state.update(metadata)
            state['playing'] = self.get_playback_status(self.active_player)
            state['position_percent'] = self.get_position(self.active_player)

        return state

    def run(self):
        """Run daemon"""
        try:
            while True:
                # 1. Get and output state
                state = self.get_state()
                print(json.dumps(state), flush=True)

                # 2. Wait for signal OR timeout
                # If signal comes, wait returns true immediately -> Loop runs again -> Fast update
                # If no signal, it waits 2 seconds then updates anyway
                self.update_event.wait(timeout=2.0)
                self.update_event.clear()

        except KeyboardInterrupt:
            pass
        except Exception as e:
            # Last ditch error catching
            error_state = {"active_player": "Error", "title": str(e)}
            print(json.dumps(error_state), flush=True)
        finally:
            self.cleanup()

if __name__ == "__main__":
    # Mode selection
    if len(sys.argv) > 1 and sys.argv[1] == "listen":
        daemon = MusicDaemon()
        daemon.run()
    else:
        daemon = MusicDaemon()
        print(json.dumps(daemon.get_state()))
