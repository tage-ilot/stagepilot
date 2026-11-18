-- ProPresenter Link: can this program control other programs on screen (macOS Accessibility)?
-- Asks System Events one harmless question and changes nothing. Prints OK, or ERR|check|<reason>.
on run argv
	try
		tell application "System Events" to set n to count of processes
		return "OK|" & n
	on error msg number code
		return "ERR|check|" & msg
	end try
end run
