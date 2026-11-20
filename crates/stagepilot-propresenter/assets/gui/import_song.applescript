-- ProPresenter Link: import ONE song through ProPresenter's own Search (MultiTracks source) on screen.
-- Arguments: 1 the song's MultiTracks number (digits only), 2 the name to give the song (no dots), 3 the playlist
-- to add it to ("" for none). The values are only ever typed into ProPresenter's own fields.
-- It touches only: the Search button, the Search panel (its MultiTracks choice, its text field, its one result row,
-- its close button) and the Import dialog (Name, Library read only, Playlist, Import, Cancel). Nothing else.
-- At any unexpected thing it stops, presses Cancel on an Import dialog if one is open, closes the Search panel
-- and prints ERR|<step>|<reason>. On success it prints OK|<name>.pro once the Import dialog has gone.

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
				keystroke "a" using command down
				keystroke songId
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
				repeat with e in (entire contents of dlg)
					try
						set r to role of e
						set n to my nameOf(e)
						if r is "AXTextField" then
							if firstField is missing value then set firstField to e
							if nameField is missing value and (n contains "|Name") then set nameField to e
						else if r is "AXPopUpButton" then
							if libraryPopup is missing value and (n contains "Library") then set libraryPopup to e
							if playlistPopup is missing value and (n contains "Playlist") then set playlistPopup to e
						end if
					end try
				end repeat
				if nameField is missing value then set nameField to firstField
				if nameField is missing value then error "The Import dialog has no Name field."
				if libraryPopup is missing value then error "The Import dialog has no Library choice."
				if (value of libraryPopup as text) is not "Songs" then error "The Import dialog's Library is not Songs."
				set focused of nameField to true
				delay 0.3
				keystroke "a" using command down
				keystroke songName
				delay 0.3
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
