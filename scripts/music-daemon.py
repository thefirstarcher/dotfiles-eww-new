#!/usr/bin/env python3
import subprocess
import json
import time
import os
import sys


class MusicDaemon:
    def __init__(self):
        self.active_player = None
        self.switch_file = "/tmp/eww-music-player-switch"

    def get_players(self):
        try:
            result = subprocess.run(
                ["playerctl", "-l"], capture_output=True, text=True, timeout=2
            )
            players = result.stdout.strip().split("\n") if result.stdout.strip() else []
            return [p for p in players if p]
        except Exception:
            return []

    def get_metadata(self, player):
        try:
            result = subprocess.run(
                [
                    "playerctl",
                    "-p",
                    player,
                    "metadata",
                    "--format",
                    "{{title}}|||{{artist}}|||{{mpris:artUrl}}|||{{mpris:length}}",
                ],
                capture_output=True,
                text=True,
                timeout=1,
            )
            if result.returncode != 0:
                return None
            parts = result.stdout.strip().split("|||")
            art_url = parts[2] if len(parts) > 2 else ""
            length = parts[3] if len(parts) > 3 and parts[3] else "0"
            if art_url.startswith("file://"):
                file_path = art_url.replace("file://", "")
                art_url = file_path if os.path.exists(file_path) else ""
            return {
                "title": parts[0] if len(parts) > 0 else "",
                "artist": parts[1] if len(parts) > 1 else "",
                "art_url": art_url,
                "length_micros": int(length),
            }
        except Exception:
            return None

    def get_playback_status(self, player):
        try:
            result = subprocess.run(
                ["playerctl", "-p", player, "status"],
                capture_output=True,
                text=True,
                timeout=1,
            )
            return result.stdout.strip() == "Playing"
        except Exception:
            return False

    def get_position_data(self, player, length_micros):
        try:
            pos_result = subprocess.run(
                ["playerctl", "-p", player, "position"],
                capture_output=True,
                text=True,
                timeout=1,
            )
            pos_sec = float(pos_result.stdout.strip() or 0)
            duration_sec = length_micros / 1000000
            percent = int((pos_sec / duration_sec) * 100) if duration_sec > 0 else 0

            def fmt(s):
                m = int(s // 60)
                sec = int(s % 60)
                return f"{m}:{sec:02d}"

            return percent, fmt(pos_sec), fmt(duration_sec)
        except Exception:
            return 0, "0:00", "0:00"

    def check_switch_request(self):
        if os.path.exists(self.switch_file):
            try:
                with open(self.switch_file, "r") as f:
                    new_player = f.read().strip()
                os.remove(self.switch_file)
                return new_player
            except:
                pass
        return None

    def get_state(self):
        available_players = self.get_players()
        switch_to = self.check_switch_request()
        if switch_to and switch_to in available_players:
            self.active_player = switch_to
        if not self.active_player or self.active_player not in available_players:
            self.active_player = available_players[0] if available_players else None

        state = {
            "active_player": self.active_player or "",
            "available_players": available_players,
            "title": "",
            "artist": "",
            "art_url": "",
            "playing": False,
            "position_percent": 0,
            "position_time": "0:00",
            "duration_time": "0:00",
        }

        if self.active_player:
            metadata = self.get_metadata(self.active_player)
            if metadata:
                state.update(metadata)
                pct, pos_str, dur_str = self.get_position_data(
                    self.active_player, metadata.get("length_micros", 0)
                )
                state["position_percent"] = pct
                state["position_time"] = pos_str
                state["duration_time"] = dur_str
            state["playing"] = self.get_playback_status(self.active_player)
        return state

    def run(self):
        while True:
            try:
                print(json.dumps(self.get_state()), flush=True)
                time.sleep(1)
            except KeyboardInterrupt:
                break
            except Exception:
                time.sleep(1)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "listen":
        MusicDaemon().run()
    else:
        print(json.dumps(MusicDaemon().get_state()))
