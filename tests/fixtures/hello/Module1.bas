Attribute VB_Name = "Module1"
Option Explicit

Public Function Add(ByVal a As Long, ByVal b As Long) As Long
    Add = a + b
End Function

Sub Main()
    Dim x As Long
    x = Add(2, 3)
End Sub
