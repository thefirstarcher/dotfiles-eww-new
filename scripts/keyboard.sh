#!/bin/bash
# ============================================================================
# Keyboard Layout Script for EWW
# ============================================================================
# Returns JSON with current keyboard layout and flag icon

get_layout() {
  # Get current keyboard layout from Sway
  if command -v swaymsg &> /dev/null; then
    layout=$(swaymsg -t get_inputs | jq -r '.[] | select(.type == "keyboard") | .xkb_active_layout_name' | head -1)

    # If full layout name, extract just the language code
    if [ -n "$layout" ]; then
      # Try to extract language code (first 2-3 letters, case insensitive)
      layout_code=$(echo "$layout" | grep -oiP '^[a-z]{2,3}' | tr '[:upper:]' '[:lower:]' | head -1)
      if [ -z "$layout_code" ]; then
        # Fallback: check for common patterns (case insensitive)
        layout_lower=$(echo "$layout" | tr '[:upper:]' '[:lower:]')
        case "$layout_lower" in
          *"english"*|*"us"*) layout_code="us" ;;
          *"ukrain"*) layout_code="ua" ;;
          *"russian"*) layout_code="ru" ;;
          *"german"*) layout_code="de" ;;
          *"french"*) layout_code="fr" ;;
          *"spanish"*) layout_code="es" ;;
          *"italian"*) layout_code="it" ;;
          *"polish"*) layout_code="pl" ;;
          *"portuguese"*) layout_code="pt" ;;
          *"dutch"*) layout_code="nl" ;;
          *"swedish"*) layout_code="se" ;;
          *"norwegian"*) layout_code="no" ;;
          *"danish"*) layout_code="dk" ;;
          *"finnish"*) layout_code="fi" ;;
          *"turkish"*) layout_code="tr" ;;
          *"arabic"*) layout_code="sa" ;;
          *"hebrew"*) layout_code="il" ;;
          *"greek"*) layout_code="gr" ;;
          *"japanese"*) layout_code="jp" ;;
          *"korean"*) layout_code="kr" ;;
          *"chinese"*) layout_code="cn" ;;
          *"belarusian"*|*"belarus"*) layout_code="by" ;;
          *) layout_code="us" ;;
        esac
      fi
    else
      layout_code="us"
    fi
  else
    # Fallback to setxkbmap for X11
    if command -v setxkbmap &> /dev/null; then
      layout_code=$(setxkbmap -query | grep layout | awk '{print $2}')
    else
      layout_code="us"
    fi
  fi

  # Generate flag icon dynamically from layout code
  # Special mappings for non-standard codes and 3-letter ISO 639-2 codes
  case "$layout_code" in
    # 2-letter mappings
    uk) layout_code="gb" ;;  # UK -> GB
    en) layout_code="us" ;;  # English -> US
    ar) layout_code="sa" ;;  # Arabic -> Saudi Arabia
    # 3-letter language codes (ISO 639-2) to 2-letter country codes (ISO 3166-1)
    eng) layout_code="us" ;;  # English -> US
    ukr) layout_code="ua" ;;  # Ukrainian -> UA
    rus) layout_code="ru" ;;  # Russian -> RU
    deu|ger) layout_code="de" ;;  # German -> DE
    fra|fre) layout_code="fr" ;;  # French -> FR
    spa) layout_code="es" ;;  # Spanish -> ES
    ita) layout_code="it" ;;  # Italian -> IT
    pol) layout_code="pl" ;;  # Polish -> PL
    por) layout_code="pt" ;;  # Portuguese -> PT
    nld|dut) layout_code="nl" ;;  # Dutch -> NL
    swe) layout_code="se" ;;  # Swedish -> SE
    nor) layout_code="no" ;;  # Norwegian -> NO
    dan) layout_code="dk" ;;  # Danish -> DK
    fin) layout_code="fi" ;;  # Finnish -> FI
    tur) layout_code="tr" ;;  # Turkish -> TR
    ara) layout_code="sa" ;;  # Arabic -> SA
    bel) layout_code="by" ;;  # Belarusian -> BY
    he) layout_code="il" ;;  # Hebrew -> Israel
    el) layout_code="gr" ;;  # Greek -> Greece
    ja) layout_code="jp" ;;  # Japanese -> Japan
    ko) layout_code="kr" ;;  # Korean -> South Korea
    zh) layout_code="cn" ;;  # Chinese -> China
    sv) layout_code="se" ;;  # Swedish -> Sweden
    da) layout_code="dk" ;;  # Danish -> Denmark
    cs) layout_code="cz" ;;  # Czech -> Czech Republic
    et) layout_code="ee" ;;  # Estonian -> Estonia
    lv) layout_code="lv" ;;  # Latvian -> Latvia
    lt) layout_code="lt" ;;  # Lithuanian -> Lithuania
    sl) layout_code="si" ;;  # Slovenian -> Slovenia
    hr) layout_code="hr" ;;  # Croatian -> Croatia
    sr) layout_code="rs" ;;  # Serbian -> Serbia
    bs) layout_code="ba" ;;  # Bosnian -> Bosnia
    mk) layout_code="mk" ;;  # Macedonian -> North Macedonia
    sq) layout_code="al" ;;  # Albanian -> Albania
    is) layout_code="is" ;;  # Icelandic -> Iceland
    fo) layout_code="fo" ;;  # Faroese -> Faroe Islands
    vi) layout_code="vn" ;;  # Vietnamese -> Vietnam
    th) layout_code="th" ;;  # Thai -> Thailand
    hi) layout_code="in" ;;  # Hindi -> India
    bn) layout_code="bd" ;;  # Bengali -> Bangladesh
    ta) layout_code="lk" ;;  # Tamil -> Sri Lanka
    fa) layout_code="ir" ;;  # Persian -> Iran
    ur) layout_code="pk" ;;  # Urdu -> Pakistan
    kk) layout_code="kz" ;;  # Kazakh -> Kazakhstan
    uz) layout_code="uz" ;;  # Uzbek -> Uzbekistan
    ky) layout_code="kg" ;;  # Kyrgyz -> Kyrgyzstan
    mn) layout_code="mn" ;;  # Mongolian -> Mongolia
    ka) layout_code="ge" ;;  # Georgian -> Georgia
    hy) layout_code="am" ;;  # Armenian -> Armenia
    az) layout_code="az" ;;  # Azerbaijani -> Azerbaijan
    be) layout_code="by" ;;  # Belarusian -> Belarus (conflict with Belgium, prefer BY)
    af) layout_code="za" ;;  # Afrikaans -> South Africa
    am) layout_code="et" ;;  # Amharic -> Ethiopia (conflict with Armenia)
    my) layout_code="mm" ;;  # Burmese -> Myanmar
    km) layout_code="kh" ;;  # Khmer -> Cambodia
    lo) layout_code="la" ;;  # Lao -> Laos
    ne) layout_code="np" ;;  # Nepali -> Nepal
    si_lk) layout_code="lk" ;;  # Sinhala -> Sri Lanka
  esac

  # Convert 2-letter country code to flag emoji using Unicode Regional Indicators
  # Regional Indicator Symbols: U+1F1E6 (A) to U+1F1FF (Z)
  if [[ "$layout_code" =~ ^[a-z]{2}$ ]]; then
    # Convert to uppercase
    upper_code=$(echo "$layout_code" | tr '[:lower:]' '[:upper:]')

    # Get first letter and convert to regional indicator
    char1="${upper_code:0:1}"
    char2="${upper_code:1:1}"

    # Calculate Unicode codepoints
    # A=65, Regional Indicator A=127462 (0x1F1E6)
    ord1=$(printf '%d' "'$char1")
    ord2=$(printf '%d' "'$char2")

    # Convert to regional indicator symbols
    ri1=$((127462 + ord1 - 65))
    ri2=$((127462 + ord2 - 65))

    # Generate flag emoji using printf with Unicode escapes
    icon=$(printf "\U$(printf '%x' $ri1)\U$(printf '%x' $ri2)")
  else
    # Fallback for non-standard codes or longer codes
    icon="⌨️ $layout_code"
  fi

  echo "{\"layout\": \"$layout_code\", \"icon\": \"$icon\"}"
}

case "$1" in
  "listen")
    # Listen mode: output current layout and monitor for changes
    get_layout

    # Monitor sway input events for keyboard layout changes
    if command -v swaymsg &> /dev/null; then
      swaymsg -t subscribe -m '["input"]' 2>/dev/null | while read -r event; do
        # Check if this is a keyboard layout change event
        if echo "$event" | grep -q "xkb_active_layout"; then
          get_layout
        fi
      done
    else
      # Fallback to polling every 0.5s for X11 (faster than 1s)
      while true; do
        sleep 0.5
        get_layout
      done
    fi
    ;;
  *)
    # Default: just get current layout
    get_layout
    ;;
esac
