-- ProPresenter Link: import ONE song through ProPresenter's own Search (MultiTracks source) on screen.
-- Arguments: 1 the song's MultiTracks number (digits only), 2 the name to give the song (no dots), 3 the playlist
-- to add it to ("" for none). The values are only ever typed into ProPresenter's own fields.
-- It touches only: the Search button, the Search panel (its MultiTracks choice, its text field, its one result row,
-- its close button) and the Import dialog (Name, Library read only, Playlist, Import, Cancel). Nothing else.
-- At any unexpected thing it stops, presses Cancel on an Import dialog if one is open, closes the Search panel
-- and prints ERR|<step>|<reason>. On success it prints OK|<name>.pro once the Import dialog has gone.
-- Nothing is typed blind: before EVERY keystroke the script checks that ProPresenter is the front app and that the
-- field meant to receive it has the keyboard (assertFocus); after typing it reads the field back and it must hold exactly
-- what was typed (the song number; the name), the Import dialog's other text fields must still hold what they held before
-- and its Library must still read Songs. Any mismatch stops with ERR (Cancel is pressed if the Import dialog is open,
-- the Search panel is closed) and nothing more is typed or pressed. At most 40 characters of what a field held are printed.

on nameOf(e)
	set t to ""
	repeat with a in {"AXTitle", "AXDescription", "AXLabel", "AXValue"}
		try
			set v to (value of attribute (a as text) of e) as text
			if v is not "" and v is not "missing value" then set t to t & "|" & v
		end try
	end repeat
	return t
end nameOf

-- The text of a field (its value), "" when it has none.
on textOf(e)
	try
		tell application "System Events" to set v to value of e
		if v is missing value then return ""
		return v as text
	on error
		return ""
	end try
end textOf

-- Exactly the same text: letters in the same case (AppleScript compares without case unless told to).
on sameText(a, b)
	considering case
		return ((a as text) is equal to (b as text))
	end considering
end sameText

-- At most 40 characters, on one line, safe to print: control characters become ? and | becomes /.
on shortText(v)
	set t to v as text
	set out to ""
	set n to count of characters of t
	if n > 40 then set n to 40
	repeat with i from 1 to n
		set c to character i of t
		set charCode to id of c
		if charCode < 32 or charCode is 127 then
			set out to out & "?"
		else if c is "|" then
			set out to out & "/"
		else
			set out to out & c
		end if
	end repeat
	if (count of characters of t) > 40 then set out to out & "..."
	return out
end shortText

-- Before every keystroke: ProPresenter is the front app and `target` is the element that has the keyboard. Else an error,
-- so nothing is typed anywhere.
on assertFocus(procName, target, what)
	set isFront to false
	set hasFocus to false
	tell application "System Events"
		try
			set isFront to (frontmost of process procName)
		end try
		try
			set hasFocus to (focused of target)
		end try
	end tell
	if isFront is not true then error "ProPresenter is not the front app, so nothing was typed."
	if hasFocus is not true then error "The " & what & " does not have the keyboard, so nothing was typed."
end assertFocus

-- After typing the name: the name field holds exactly the name, the other text fields hold what they held before, and the
-- Library still reads Songs.
on verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
	set held to my textOf(item nameIndex of textFields)
	if not my sameText(held, songName) then error "The Name field held \"" & my shortText(held) & "\" instead of the name. Nothing was imported."
	repeat with i from 1 to (count of textFields)
		if i is not nameIndex then
			set nowHeld to my textOf(item i of textFields)
			if not my sameText(nowHeld, item i of heldBefore) then error "Another field of the Import dialog changed to \"" & my shortText(nowHeld) & "\". Nothing was imported."
		end if
	end repeat
	if not my sameText(my textOf(libraryPopup), "Songs") then error "The Import dialog's Library no longer reads Songs. Nothing was imported."
end verifyDialog

on findWindow(procName, wantedSubrole)
	tell application "System Events"
		tell process procName
			repeat with w in windows
				try
					if subrole of w is wantedSubrole then return w
				end try
			end repeat
		end tell
	end tell
	return missing value
end findWindow

on waitForWindow(procName, wantedSubrole, seconds)
	repeat (seconds * 4) times
		set w to my findWindow(procName, wantedSubrole)
		if w is not missing value then return w
		delay 0.25
	end repeat
	return missing value
end waitForWindow

on resultTable(fw)
	tell application "System Events"
		try
			return table 1 of scroll area 1 of group 3 of group 1 of fw
		end try
		repeat with e in (entire contents of fw)
			try
				if role of e is "AXTable" then return e
			end try
		end repeat
	end tell
	return missing value
end resultTable

on closeEverything(procName)
	tell application "System Events"
		tell process procName
			set dlg to my findWindow(procName, "AXDialog")
			if dlg is not missing value then
				try
					click button "Cancel" of dlg
					delay 0.5
				end try
			end if
			set fw to my findWindow(procName, "AXFloatingWindow")
			if fw is not missing value then
				try
					repeat with b in (buttons of fw)
						if subrole of b is "AXCloseButton" then
							click b
							exit repeat
						end if
					end repeat
				end try
			end if
		end tell
	end tell
end closeEverything

on run argv
	set procName to "ProPresenter"
	set stepName to "start"
	try
		if (count of argv) is not 3 then error "The script needs the song number, the name and the playlist."
		set songId to item 1 of argv
		set songName to item 2 of argv
		set playlistName to item 3 of argv
		if songId is "" or songId contains " " then error "The song number is not valid."
		repeat with ch in (characters of songId)
			if "0123456789" does not contain (ch as text) then error "The song number is not valid."
		end repeat
		if songName is "" or songName contains "." then error "The song name is not valid."

		set stepName to "find ProPresenter"
		tell application "System Events"
			if not (exists process procName) then error "ProPresenter is not running."
		end tell

		set stepName to "open the Search panel"
		tell application "System Events"
			tell process procName
				set clicked to false
				try
					click button "Search" of toolbar 1 of window "ProPresenter"
					set clicked to true
				end try
				if not clicked then
					repeat with e in (entire contents of window "ProPresenter")
						try
							if role of e is "AXButton" and ((title of e as text) is "Search" or (description of e as text) is "Search") then
								click e
								set clicked to true
								exit repeat
							end if
						end try
					end repeat
				end if
				if not clicked then error "The Search button was not found."
			end tell
		end tell
		set fw to my waitForWindow(procName, "AXFloatingWindow", 6)
		if fw is missing value then error "The Search panel did not open."

		set stepName to "choose the MultiTracks source"
		tell application "System Events"
			tell process procName
				set chosen to false
				repeat with e in (entire contents of fw)
					try
						set r to role of e
						set n to my nameOf(e)
						if n contains "MultiTracks" then
							if r is "AXRadioButton" or r is "AXButton" or r is "AXMenuItem" or r is "AXTab" then
								click e
								set chosen to true
								exit repeat
							else if r is "AXPopUpButton" then
								if (value of e as text) contains "MultiTracks" then
									set chosen to true
								end if
								exit repeat
							end if
						end if
					end try
				end repeat
				if not chosen then
					repeat with e in (entire contents of fw)
						try
							if role of e is "AXPopUpButton" then
								click e
								delay 0.4
								click (first menu item of menu 1 of e whose name contains "MultiTracks")
								set chosen to true
								exit repeat
							end if
						end try
					end repeat
				end if
				if not chosen then error "The MultiTracks source was not found in the Search panel."
			end tell
		end tell
		delay 0.4

		set stepName to "type the song number"
		tell application "System Events"
			tell process procName
				set field to missing value
				repeat with e in (entire contents of fw)
					try
						if role of e is "AXTextField" or role of e is "AXSearchField" or role of e is "AXComboBox" then
							set field to e
							exit repeat
						end if
					end try
				end repeat
				if field is missing value then error "The Search panel has no text field."
				set focused of field to true
				delay 0.3
				my assertFocus(procName, field, "Search field")
				keystroke "a" using command down
				my assertFocus(procName, field, "Search field")
				keystroke songId
				delay 0.3
				set held to my textOf(field)
				if not my sameText(held, songId) then error "The Search field held \"" & my shortText(held) & "\" instead of the song number. Nothing else was typed or pressed."
			end tell
		end tell

		set stepName to "read the search result"
		set rowCount to 0
		set stable to 0
		repeat 24 times
			delay 0.5
			set t to my resultTable(fw)
			if t is not missing value then
				tell application "System Events" to set n to count of (rows of t)
				if n is rowCount and n > 0 then
					set stable to stable + 1
				else
					set stable to 0
				end if
				set rowCount to n
				if stable >= 3 then exit repeat
			end if
		end repeat
		if rowCount is 0 then error "The search found no song with this number."
		if rowCount is not 1 then error "The search found " & rowCount & " songs for this number, not exactly one."

		set stepName to "open the Import dialog"
		tell application "System Events"
			tell process procName
				select row 1 of (my resultTable(fw))
				delay 0.6
				try
					set focused of (my resultTable(fw)) to true
				end try
				delay 0.2
				my assertFocus(procName, (my resultTable(fw)), "results list")
				key code 36
			end tell
		end tell
		set dlg to my waitForWindow(procName, "AXDialog", 8)
		if dlg is missing value then error "The Import dialog did not open."

		set stepName to "fill in the Import dialog"
		tell application "System Events"
			tell process procName
				set nameField to missing value
				set firstField to missing value
				set libraryPopup to missing value
				set playlistPopup to missing value
				set textFields to {}
				set nameIndex to 0
				repeat with e in (entire contents of dlg)
					try
						set r to role of e
						set n to my nameOf(e)
						if r is "AXTextField" then
							set end of textFields to e
							if firstField is missing value then set firstField to e
							if nameField is missing value and (n contains "|Name") then
								set nameField to e
								set nameIndex to count of textFields
							end if
						else if r is "AXPopUpButton" then
							if libraryPopup is missing value and (n contains "Library") then set libraryPopup to e
							if playlistPopup is missing value and (n contains "Playlist") then set playlistPopup to e
						end if
					end try
				end repeat
				if nameField is missing value then
					set nameField to firstField
					set nameIndex to 1
				end if
				if nameField is missing value then error "The Import dialog has no Name field."
				if libraryPopup is missing value then error "The Import dialog has no Library choice."
				if (value of libraryPopup as text) is not "Songs" then error "The Import dialog's Library is not Songs."
				-- What every text field holds before anything is typed.
				set heldBefore to {}
				repeat with f in textFields
					set end of heldBefore to my textOf(f)
				end repeat
				set focused of nameField to true
				delay 0.3
				my assertFocus(procName, nameField, "Name field")
				keystroke "a" using command down
				my assertFocus(procName, nameField, "Name field")
				keystroke songName
				delay 0.3
				my verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
				if playlistName is not "" then
					if playlistPopup is missing value then error "The Import dialog has no Playlist choice."
					click playlistPopup
					delay 0.5
					click (first menu item of menu 1 of playlistPopup whose name is playlistName)
					delay 0.4
				else if playlistPopup is not missing value then
					try
						if (value of playlistPopup as text) is not "None" then
							click playlistPopup
							delay 0.5
							click (first menu item of menu 1 of playlistPopup whose name is "None")
							delay 0.4
						end if
					end try
				end if
			end tell
		end tell

		set stepName to "press Import"
		tell application "System Events"
			tell process procName
				-- Once more, after the Playlist choice: what is about to be imported is what was asked for.
				my verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
				click button "Import" of dlg
			end tell
		end tell

		set stepName to "wait for the import to finish"
		repeat 120 times
			delay 0.25
			if my findWindow(procName, "AXDialog") is missing value then exit repeat
		end repeat
		if my findWindow(procName, "AXDialog") is not missing value then error "The Import dialog stayed open."

		set stepName to "close the Search panel"
		my closeEverything(procName)
		return "OK|" & songName & ".pro"
	on error msg
		try
			my closeEverything(procName)
		end try
		return "ERR|" & stepName & "|" & msg
	end try
end run
