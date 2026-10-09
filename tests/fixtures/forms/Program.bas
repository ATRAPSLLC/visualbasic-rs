Attribute VB_Name = "Program"
Option Explicit

' Calls a form's public members from a module: through a New instance and
' through the predeclared one.

Sub Main()
    Dim f As Board, n As Long
    Set f = New Board
    Load f
    f.Reset 5
    f.Title = "Forms"
    n = f.Clicks + Len(f.Title)
    Board.Reset n
    f.Show
    Unload f
    Set f = Nothing
End Sub
