# lazyllm demo — generates demo.gif for the README
# Prerequisites: cargo build --release && ollama pull tinyllama
# Run with: vhs demo.tape

Output demo/demo.gif
Set FontSize 16
Set Width 750
Set Height 500
Set Padding 5
Set Theme "Catppuccin Mocha"
Set TypingSpeed 75ms
Set Shell zsh

# Launch the app silently — hide the command from the recording
Hide
Type "./target/release/lazyllm"
Enter
Sleep 3s
Show

# Pause to show the UI
Sleep 1s

# First chat: enter insert mode and say hello
Type "i"
Sleep 400ms
Type "Say hello!"
Sleep 800ms
Enter
Sleep 5s

# Back to normal mode
Escape
Sleep 600ms

# Focus to chat list and create a second chat
Type "h"
Sleep 500ms
Type "n"
Sleep 800ms

# Focus to input and send a message in the second chat
Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "i"
Sleep 400ms
Type "What is 2 + 2?"
Sleep 800ms
Enter
Sleep 5s

# Back to normal and toggle between chats
Escape
Sleep 500ms
Type "h"
Sleep 500ms
Type "k"
Sleep 800ms
Enter
Sleep 1s

# Back to second chat
Type "j"
Sleep 800ms
Enter
Sleep 1s

# Focus back to chat view
Type "l"
Sleep 500ms

# Open help overlay
Type "?"
Sleep 2s
Type "?"
Sleep 500ms

# Open model selector popup
Type "m"
Sleep 2s
Escape
Sleep 500ms

# Delete the current chat
Type "h"
Sleep 500ms
Type "d"
Sleep 1s

# Quit — hidden so the recording ends on the app
Hide
Type "q"
Sleep 500ms
