-- ProPresenter Link: find the row of one library in ProPresenter's library list (the left side of its main window)
-- and say where it is on screen. It clicks nothing itself: the program that runs it clicks that spot once with a real
-- mouse event (click.swift), because AppleScript's own `click at` did not land on this list.
-- Argument 1: the library's name. Prints FOUND|x|y (the middle of the row's name), or ERR|<step>|<reason>.
on run argv
	set stepName to "start"
	try
		set target to item 1 of argv
		set stepName to "find ProPresenter"
		tell application "System Events"
			if not (exists process "ProPresenter") then error "ProPresenter is not running."
			tell process "ProPresenter"
				set stepName to "find the library row"
				set w to window "ProPresenter"
				repeat with e in (entire contents of w)
					try
						if role of e is "AXStaticText" and (value of e as text) is target then
							set p to position of e
							set s to size of e
							set cx to (item 1 of p) + (item 1 of s) / 2
							set cy to (item 2 of p) + (item 2 of s) / 2
							return "FOUND|" & (round cx) & "|" & (round cy)
						end if
					end try
				end repeat
			end tell
		end tell
		error "The library " & target & " is not in ProPresenter's list."
	on error msg
		return "ERR|" & stepName & "|" & msg
	end try
end run
