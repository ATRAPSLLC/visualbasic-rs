Attribute VB_Name = "Program"
Option Explicit

' Drives the form's public members.

Sub Main()
    Dim s As String
    Load Client
    Client.Start "localhost", 80
    s = Client.Fetch("http://localhost/") & Client.PickFile()
    Unload Client
End Sub
