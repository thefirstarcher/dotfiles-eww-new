#!/usr/bin/env python3
# ============================================================================
# Music Player Daemon for EWW Dashboard
# ============================================================================
# Monitors MPRIS players via playerctl and allows switching between them

import subprocess
import json
import time
import os
import sys

class MusicDaemon:
    def __init__(self):
        self.active_player = None
        self.switch_file = '/tmp/eww-music-player-switch'

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
            return [p for p in players if p]  # Filter empty strings
        except Exception:
            return []

    def get_metadata(self, player):
        """Get metadata for a specific player"""
        try:
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
                # Check if file exists
                if os.path.exists(file_path):
                    art_url = file_path  # Use path without file:// prefix
                else:
                    art_url = ""  # File doesn't exist, clear it

            return {
                'title': parts[0] if len(parts) > 0 else "",
                'artist': parts[1] if len(parts) > 1 else "",
                'art_url': art_url
            }
        except Exception:
            return None

    def get_playback_status(self, player):
        """Get playback status for a player"""
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
        """Get position percentage for a player"""
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

            pos = float(pos_result.stdout.strip())
            dur = int(dur_result.stdout.strip()) / 1000000  # Convert microseconds to seconds

            return int((pos / dur) * 100) if dur > 0 else 0
        except Exception:
            return 0

    def check_switch_request(self):
        """Check if user requested to switch player"""
        if os.path.exists(self.switch_file):
            try:
                with open(self.switch_file, 'r') as f:
                    new_player = f.read().strip()
                    os.remove(self.switch_file)
                    return new_player
            except Exception:
                pass
        return None

    def get_state(self):
        """Get current state of all players"""
        # Get available players
        available_players = self.get_players()

        # Check for manual switch
        switch_to = self.check_switch_request()
        if switch_to and switch_to in available_players:
            self.active_player = switch_to

        # Auto-select first player if none active
        if not self.active_player or self.active_player not in available_players:
            self.active_player = available_players[0] if available_players else None

        # Build state object
        state = {
            "active_player": self.active_player or "",
            "available_players": available_players,
            "title": "",
            "artist": "",
            "art_url": "",
            "playing": False,
            "position_percent": 0
        }

        # Get metadata for active player
        if self.active_player:
            metadata = self.get_metadata(self.active_player)
            if metadata:
                state.update(metadata)

            state['playing'] = self.get_playback_status(self.active_player)
            state['position_percent'] = self.get_position(self.active_player)

        return state

    def run(self):
        """Run daemon - output state continuously"""
        while True:
            try:
                state = self.get_state()
                print(json.dumps(state), flush=True)
                time.sleep(1)
            except KeyboardInterrupt:
                break
            except Exception as e:
                # Output error state but keep running
                error_state = {
                    "active_player": "",
                    "available_players": [],
                    "title": "",
                    "artist": "",
                    "art_url": "",
                    "playing": False,
                    "position_percent": 0
                }
                print(json.dumps(error_state), flush=True)
                time.sleep(1)

if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "listen":
        daemon = MusicDaemon()
        daemon.run()
    else:
        # One-shot mode
        daemon = MusicDaemon()
        print(json.dumps(daemon.get_state()))
