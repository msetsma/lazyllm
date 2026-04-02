# lazyllm demo — generates demo.gif for the README
# Prerequisites: cargo build --release && ollama pull tinyllama
# Run with: vhs demo.tape

Output demo/demo.gif
Set FontSize 16
Set Width 900
Set Height 550
Set Padding 5
Set Theme "Catppuccin Mocha"
Set TypingSpeed 65ms
Set Shell zsh

# Launch the app silently — hide the command from the recording
Hide
Type "./target/release/lazyllm"
Enter
Sleep 3s
Show

# Pause to show the initial UI
Sleep 1s

# ── Build up context with two exchanges ───────────────────
Type "i"
Sleep 400ms
Type "What is Rust's ownership model? Keep it brief."
Sleep 800ms
Enter
Sleep 9s

Escape
Sleep 500ms

Type "i"
Sleep 300ms
Type "Show a quick borrow example in code."
Sleep 700ms
Enter
Sleep 9s

Escape
Sleep 500ms

# ── Pulse overlay: context budget & session stats ─────────
Type "P"
Sleep 1.5s

# Scroll down through session stats, pinned messages, notes
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms

# Cycle compaction mode with 's' to show profile options
Type "s"
Sleep 900ms
Type "s"
Sleep 900ms
Type "s"
Sleep 700ms

# Scroll back up to the budget view
Type "k"
Sleep 500ms
Type "k"
Sleep 500ms
Type "k"
Sleep 500ms

# Close Pulse
Type "P"
Sleep 800ms

# ── Visual mode: highlight and copy a message ─────────────
Type "v"
Sleep 900ms
Type "k"
Sleep 700ms
Type "y"
Sleep 1.2s
Escape
Sleep 700ms

# ── Command mode: toggle timestamps ───────────────────────
Type ":set timestamps on"
Enter
Sleep 1s

# ── Model switcher ────────────────────────────────────────
Type "m"
Sleep 2s
Escape
Sleep 500ms

# ── Help overlay ──────────────────────────────────────────
Type "?"
Sleep 2.5s
Escape
Sleep 500ms

# ── New chat ──────────────────────────────────────────────
Type "h"
Sleep 400ms
Type "n"
Sleep 800ms

Type "l"
Sleep 300ms
Type "i"
Sleep 300ms
Type "What is 2 + 2?"
Sleep 700ms
Enter
Sleep 6s

Escape
Sleep 500ms

# ── Switch between chats ──────────────────────────────────
Type "h"
Sleep 400ms
Type "k"
Sleep 700ms
Enter
Sleep 900ms
Type "j"
Sleep 700ms
Enter
Sleep 900ms
Type "l"
Sleep 400ms

# Quit — hidden so the recording ends cleanly on the app
Hide
Type "q"
Sleep 500ms
