Attribute VB_Name = "Program"
Option Explicit

' Late calls by name through an Object variable (LateMem*), and the form's
' calls on its UserControl (LateId*).

Private Function LateByName() As Long
    Dim o As Object, n As Long
    Set o = New Bag
    o.Size = 2
    n = o.Size
    o.Store Key:="a", Value:=1
    o.Store "b", 2
    o.Item(Index:=1) = 3
    o.Item(2) = o.Item(Index:=1)
    Set o.Slot(Index:=0) = o
    Set o.Slot(1) = Nothing
    LateByName = n + o.Size
End Function

Sub Main()
    Dim n As Double, s As String
    n = LateByName()
    Load Host
    n = n + Host.Properties() + Host.Methods() + Host.Extender()
    s = Host.Indexed() & Host.Named()
    Unload Host
End Sub
