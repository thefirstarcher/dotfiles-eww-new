#!/usr/bin/env python3
"""
Pomodoro Timer Daemon
Clean architecture: 1 daemon, commands connect via Unix socket
"""

import os
import sys
import json
import time
import socket
import signal
import threading
from pathlib import Path

# Configuration
WORK_DURATION = 1500  # 25 minutes
SHORT_BREAK = 300     # 5 minutes
LONG_BREAK = 900      # 15 minutes

# Runtime files
SOCKET_PATH = "/tmp/pomodoro.sock"
PID_FILE = "/tmp/pomodoro.pid"
STATE_FILE = "/tmp/pomodoro-state.json"


class PomodoroTimer:
    def __init__(self):
        self.status = "idle"
        self.time_left = WORK_DURATION
        self.sessions = 0
        self.is_break = False
        self.running = False
        self.duration = WORK_DURATION
        self.lock = threading.Lock()
        self.listeners = []

    def get_state(self):
        """Get current state as JSON"""
        with self.lock:
            minutes = self.time_left // 60
            seconds = self.time_left % 60
            time_display = f"{minutes}:{seconds:02d}"
            percent = ((self.duration - self.time_left) * 100 // self.duration) if self.duration > 0 else 0
            icon = "" if self.is_break else ""

            return {
                "status": self.status,
                "time_left": self.time_left,
                "time_display": time_display,
                "sessions": self.sessions,
                "is_break": self.is_break,
                "percent": percent,
                "icon": icon
            }

    def save_state(self):
        """Save state to file and notify listeners"""
        state = self.get_state()
        state_json = json.dumps(state)

        # Write to file
        with open(STATE_FILE, 'w') as f:
            f.write(state_json + '\n')

        # Notify all listeners
        dead_listeners = []
        for listener in self.listeners[:]:
            try:
                listener.sendall((state_json + '\n').encode())
            except:
                dead_listeners.append(listener)

        # Remove dead listeners
        for listener in dead_listeners:
            if listener in self.listeners:
                self.listeners.remove(listener)

    def toggle(self):
        """Toggle between running and paused"""
        with self.lock:
            if self.running:
                self.status = "paused"
                self.running = False
            else:
                if self.status == "paused":
                    # Resume
                    self.status = "running"
                    self.running = True
                else:
                    # Start new session
                    self.status = "running"
                    self.running = True
                    if self.is_break:
                        if self.sessions % 4 == 0 and self.sessions > 0:
                            self.duration = LONG_BREAK
                        else:
                            self.duration = SHORT_BREAK
                    else:
                        self.duration = WORK_DURATION
                        self.sessions += 1
                    self.time_left = self.duration
        self.save_state()

    def stop(self):
        """Stop timer and reset"""
        with self.lock:
            self.status = "idle"
            self.running = False
            self.time_left = WORK_DURATION
            self.duration = WORK_DURATION
            self.sessions = 0
            self.is_break = False
        self.save_state()

    def skip(self):
        """Skip to next session"""
        with self.lock:
            self.running = False
            self.status = "idle"
            if self.is_break:
                self.is_break = False
                self.time_left = WORK_DURATION
                self.duration = WORK_DURATION
            else:
                self.is_break = True
                if self.sessions % 4 == 0 and self.sessions > 0:
                    self.time_left = LONG_BREAK
                    self.duration = LONG_BREAK
                else:
                    self.time_left = SHORT_BREAK
                    self.duration = SHORT_BREAK
        self.save_state()

    def tick(self):
        """Timer tick - called every second"""
        with self.lock:
            if self.running and self.time_left > 0:
                self.time_left -= 1

                # Check if timer finished
                if self.time_left == 0:
                    self.running = False
                    self.status = "idle"

                    if self.is_break:
                        # Break finished, switch to work
                        os.system('notify-send "Break Over!" "Time to focus!" -u normal 2>/dev/null &')
                        self.is_break = False
                        self.time_left = WORK_DURATION
                        self.duration = WORK_DURATION
                    else:
                        # Work finished, switch to break
                        if self.sessions % 4 == 0:
                            os.system('notify-send "Pomodoro Complete!" "Take a long break!" -u normal 2>/dev/null &')
                            self.is_break = True
                            self.time_left = LONG_BREAK
                            self.duration = LONG_BREAK
                        else:
                            os.system('notify-send "Pomodoro Complete!" "Take a short break!" -u normal 2>/dev/null &')
                            self.is_break = True
                            self.time_left = SHORT_BREAK
                            self.duration = SHORT_BREAK

        self.save_state()


class PomodoroDaemon:
    def __init__(self):
        self.timer = PomodoroTimer()
        self.running = True
        self.sock = None

    def cleanup(self, signum=None, frame=None):
        """Cleanup on exit"""
        self.running = False
        if self.sock:
            self.sock.close()
        if os.path.exists(SOCKET_PATH):
            os.unlink(SOCKET_PATH)
        if os.path.exists(PID_FILE):
            os.unlink(PID_FILE)
        if os.path.exists(STATE_FILE):
            os.unlink(STATE_FILE)
        sys.exit(0)

    def handle_client(self, conn, addr):
        """Handle client connection"""
        try:
            data = conn.recv(1024).decode().strip()

            if data == "listen":
                # Add to listeners and send initial state
                self.timer.listeners.append(conn)
                state_json = json.dumps(self.timer.get_state())
                conn.sendall((state_json + '\n').encode())
                # Keep connection open for updates
            elif data == "toggle":
                self.timer.toggle()
                conn.sendall(b"OK\n")
                conn.close()
            elif data == "stop":
                self.timer.stop()
                conn.sendall(b"OK\n")
                conn.close()
            elif data == "skip":
                self.timer.skip()
                conn.sendall(b"OK\n")
                conn.close()
            elif data == "state":
                state_json = json.dumps(self.timer.get_state())
                conn.sendall((state_json + '\n').encode())
                conn.close()
            elif data == "quit":
                conn.sendall(b"OK\n")
                conn.close()
                self.cleanup()
            else:
                conn.sendall(b"UNKNOWN\n")
                conn.close()
        except Exception as e:
            try:
                conn.close()
            except:
                pass

    def timer_thread(self):
        """Background thread that ticks the timer"""
        while self.running:
            time.sleep(1)
            self.timer.tick()

    def run(self):
        """Run the daemon"""
        # Check if already running
        if os.path.exists(PID_FILE):
            try:
                with open(PID_FILE, 'r') as f:
                    pid = int(f.read().strip())
                os.kill(pid, 0)  # Check if process exists
                print(f"Daemon already running with PID {pid}", file=sys.stderr)
                sys.exit(1)
            except (OSError, ValueError):
                # Stale PID file
                os.unlink(PID_FILE)

        # Write PID file
        with open(PID_FILE, 'w') as f:
            f.write(str(os.getpid()))

        # Setup signal handlers
        signal.signal(signal.SIGTERM, self.cleanup)
        signal.signal(signal.SIGINT, self.cleanup)

        # Remove old socket
        if os.path.exists(SOCKET_PATH):
            os.unlink(SOCKET_PATH)

        # Create Unix socket
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.bind(SOCKET_PATH)
        self.sock.listen(5)

        # Start timer thread
        timer_t = threading.Thread(target=self.timer_thread, daemon=True)
        timer_t.start()

        # Save initial state
        self.timer.save_state()

        # Accept connections
        while self.running:
            try:
                conn, addr = self.sock.accept()
                # Handle in new thread
                client_t = threading.Thread(target=self.handle_client, args=(conn, addr), daemon=True)
                client_t.start()
            except Exception as e:
                if self.running:
                    print(f"Error: {e}", file=sys.stderr)

        self.cleanup()


def send_command(cmd):
    """Send a command to the daemon"""
    # Start daemon if not running
    if not os.path.exists(SOCKET_PATH):
        # Fork daemon
        pid = os.fork()
        if pid == 0:
            # Child - become daemon
            os.setsid()
            daemon = PomodoroDaemon()
            daemon.run()
            sys.exit(0)
        else:
            # Parent - wait for socket to appear
            for _ in range(50):
                if os.path.exists(SOCKET_PATH):
                    time.sleep(0.1)  # Give it a moment to start listening
                    break
                time.sleep(0.1)

    # Connect and send command
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(SOCKET_PATH)
        sock.sendall((cmd + '\n').encode())

        if cmd == "listen":
            # Stream responses
            while True:
                data = sock.recv(4096)
                if not data:
                    break
                print(data.decode(), end='', flush=True)
        else:
            # Get response
            response = sock.recv(1024).decode()
            if cmd == "state":
                print(response, end='')

        sock.close()
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        # Default: get state
        send_command("state")
    elif sys.argv[1] == "daemon":
        # Run as daemon
        daemon = PomodoroDaemon()
        daemon.run()
    elif sys.argv[1] in ["listen", "toggle", "stop", "skip", "kill", "quit"]:
        if sys.argv[1] == "kill":
            send_command("quit")
        else:
            send_command(sys.argv[1])
    else:
        print(f"Usage: {sys.argv[0]} [listen|toggle|stop|skip|kill]", file=sys.stderr)
        sys.exit(1)
