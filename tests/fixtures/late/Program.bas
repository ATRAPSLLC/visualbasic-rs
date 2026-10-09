Attribute VB_Name = "Program"
Option Explicit

Sub Main()
    Dim o As Object
    Set o = CreateObject("Scripting.Dictionary")
    o.Add "a", 1
    o.Item("b") = 2
    Dim n As Long
    n = o.Count
    Dim e As Boolean
    e = o.Exists(Key:="a")
    Dim x As Variant
    x = o.Item("a")
    o.RemoveAll
    Set o.Owner = Nothing
End Sub
