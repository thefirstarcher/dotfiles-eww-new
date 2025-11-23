#!/bin/bash
# Pomodoro Timer with Daemon Architecture

DAEMON_PID="/tmp/pomodoro-daemon.pid"
COMMAND_FIFO="/tmp/pomodoro-command.fifo"
STATE_FILE="/tmp/pomodoro-state.json"

# Durations in seconds
WORK_DURATION=1500    # 25 minutes
SHORT_BREAK=300       # 5 minutes
LONG_BREAK=900        # 15 minutes

# Daemon function
daemon() {
  # Create command FIFO
  rm -f "$COMMAND_FIFO"
  mkfifo "$COMMAND_FIFO"

  # Initial state
  local status="idle"
  local time_left=$WORK_DURATION
  local sessions=0
  local is_break=false
  local running=false
  local duration=$WORK_DURATION

  save_state() {
    local percent=0
    [ $duration -gt 0 ] && percent=$(( (duration - time_left) * 100 / duration ))
    local minutes=$((time_left / 60))
    local seconds=$((time_left % 60))
    local time_display=$(printf "%d:%02d" $minutes $seconds)
    local icon=""
    [ "$is_break" = true ] && icon="" || icon=""

    echo "{\"status\":\"$status\",\"time_left\":$time_left,\"time_display\":\"$time_display\",\"sessions\":$sessions,\"is_break\":$is_break,\"percent\":$percent,\"icon\":\"$icon\"}" > "$STATE_FILE"
  }

  # Save initial state
  save_state

  # Main daemon loop
  while true; do
    # Check for commands (non-blocking)
    if read -t 0.1 cmd < "$COMMAND_FIFO" 2>/dev/null; then
      case "$cmd" in
        toggle)
          if [ "$running" = true ]; then
            status="paused"
            running=false
          else
            # Check if we're resuming from pause or starting fresh
            if [ "$status" = "paused" ]; then
              status="running"
              running=true
              # Resume with current time
            else
              status="running"
              running=true
              # Start new session
              if [ "$is_break" = true ]; then
                if [ $((sessions % 4)) -eq 0 ] && [ $sessions -gt 0 ]; then
                  duration=$LONG_BREAK
                else
                  duration=$SHORT_BREAK
                fi
              else
                duration=$WORK_DURATION
                sessions=$((sessions + 1))
              fi
              time_left=$duration
            fi
          fi
          save_state
          ;;
        stop)
          status="idle"
          running=false
          time_left=$WORK_DURATION
          duration=$WORK_DURATION
          sessions=0
          is_break=false
          save_state
          ;;
        skip)
          running=false
          status="idle"
          if [ "$is_break" = true ]; then
            is_break=false
            time_left=$WORK_DURATION
            duration=$WORK_DURATION
          else
            is_break=true
            if [ $((sessions % 4)) -eq 0 ] && [ $sessions -gt 0 ]; then
              time_left=$LONG_BREAK
              duration=$LONG_BREAK
            else
              time_left=$SHORT_BREAK
              duration=$SHORT_BREAK
            fi
          fi
          save_state
          ;;
        quit)
          rm -f "$COMMAND_FIFO" "$STATE_FILE" "$DAEMON_PID"
          exit 0
          ;;
      esac
    fi

    # Timer tick
    if [ "$running" = true ] && [ $time_left -gt 0 ]; then
      sleep 1
      time_left=$((time_left - 1))
      save_state

      # Timer finished
      if [ $time_left -eq 0 ]; then
        running=false
        status="idle"

        if [ "$is_break" = true ]; then
          notify-send "Break Over!" "Time to focus!" -u normal
          is_break=false
          time_left=$WORK_DURATION
          duration=$WORK_DURATION
        else
          if [ $((sessions % 4)) -eq 0 ]; then
            notify-send "Pomodoro Complete!" "Take a long break!" -u normal
            is_break=true
            time_left=$LONG_BREAK
            duration=$LONG_BREAK
          else
            notify-send "Pomodoro Complete!" "Take a short break!" -u normal
            is_break=true
            time_left=$SHORT_BREAK
            duration=$SHORT_BREAK
          fi
        fi
        save_state
      fi
    elif [ "$running" = false ]; then
      # Just wait when not running
      sleep 0.5
    fi
  done
}

# Start daemon if not running
start_daemon() {
  # Check if daemon is actually running by checking if FIFO is being read
  if [ -f "$DAEMON_PID" ] && [ -p "$COMMAND_FIFO" ]; then
    local pid=$(cat "$DAEMON_PID")
    if kill -0 $pid 2>/dev/null; then
      # Check if process is reading from FIFO
      if lsof "$COMMAND_FIFO" 2>/dev/null | grep -q "$pid"; then
        return 0
      fi
    fi
  fi

  # Clean up stale files
  rm -f "$DAEMON_PID" "$COMMAND_FIFO"

  # Start daemon
  daemon &
  echo $! > "$DAEMON_PID"
  disown

  # Wait for daemon to initialize
  for i in {1..50}; do
    [ -p "$COMMAND_FIFO" ] && break
    sleep 0.1
  done
}

# Send command to daemon
send_command() {
  start_daemon
  echo "$1" > "$COMMAND_FIFO"
}

# Main command handler
case "$1" in
  listen)
    # Start daemon if needed
    start_daemon

    # Output initial state
    cat "$STATE_FILE" 2>/dev/null || echo '{"status":"idle","time_left":1500,"time_display":"25:00","sessions":0,"is_break":false,"percent":0,"icon":""}'

    # Watch for changes
    if command -v inotifywait &> /dev/null; then
      while true; do
        inotifywait -q -e modify "$STATE_FILE" 2>/dev/null >/dev/null
        cat "$STATE_FILE"
      done
    else
      while sleep 1; do
        cat "$STATE_FILE"
      done
    fi
    ;;

  toggle)
    send_command "toggle"
    ;;

  stop)
    send_command "stop"
    ;;

  skip)
    send_command "skip"
    ;;

  kill)
    if [ -f "$DAEMON_PID" ]; then
      kill $(cat "$DAEMON_PID") 2>/dev/null
      rm -f "$DAEMON_PID" "$COMMAND_FIFO" "$STATE_FILE"
    fi
    ;;

  *)
    # Get current state (don't start daemon if not running)
    if [ -f "$STATE_FILE" ]; then
      cat "$STATE_FILE"
    else
      echo '{"status":"idle","time_left":1500,"time_display":"25:00","sessions":0,"is_break":false,"percent":0,"icon":""}'
    fi
    ;;
esac
