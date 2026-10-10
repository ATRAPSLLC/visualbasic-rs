Attribute VB_Name = "Program"
Option Explicit

Sub Main()
    Dim f As New First, s As New Second, q As New Quiet, t As New Third
    Debug.Print f.N + s.N
    t.Pong
End Sub
