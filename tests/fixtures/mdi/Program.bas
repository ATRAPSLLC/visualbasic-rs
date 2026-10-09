Attribute VB_Name = "Program"
Option Explicit

' Shows the MDIForm and drives a child form held in a variable and the
' predeclared instance: its public members, built-in methods with and
' without their optional arguments.

Sub Main()
    Dim f As Child, n As Long
    Frame.Show
    Set f = New Child
    Load f
    f.Reset
    f.Move 0
    f.Move 0, 0, 2000, 1000
    f.Show
    f.Show 0
    n = f.Total + Frame.Opened
    Unload f
    Set f = Nothing
    Child.Reset
    n = n + Child.Total
    Frame.OpenChild "third"
End Sub
