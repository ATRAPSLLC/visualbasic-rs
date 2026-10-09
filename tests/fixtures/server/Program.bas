Attribute VB_Name = "Program"
Option Explicit

' An ActiveX EXE: one class per Instancing value, used from Sub Main.

Sub Main()
    Dim h As Hidden, m As Multi
    Set h = New Hidden
    Set m = New Multi
    h.Value = 2
    m.Value = h.Twice()
End Sub
