Attribute VB_Name = "Program"
Option Explicit

' Reads and writes every public member of Holder from outside the class
' (each through its accessor), and calls Two and Three through each of
' their interfaces.

Private Function Members() As Double
    Dim h As Holder, p As Holder, r As Double, x As Variant
    Set h = New Holder
    Set p = New Holder
    h.B = 1
    h.I = 2
    h.L = 3
    h.S = 4.5
    h.D = 5.25
    h.C = 6.5
    h.Dt = #1/2/2003#
    h.Flag = True
    h.Str = "seven"
    h.V = 8
    h.V = "nine"
    Set h.V = p
    Set h.O = p
    Set h.Peer = p
    h.Auto.Add 10
    r = h.B + h.I + h.L + h.S + h.D + h.C + CDbl(h.Dt) + Len(h.Str)
    If h.Flag Then r = r + 1
    x = h.V
    If IsObject(h.V) Then r = r + 1
    If Not h.O Is Nothing Then r = r + 1
    r = r + h.Peer.Sum() + h.Auto.Count
    Set h.Peer = Nothing
    Members = r
End Function

Private Function Interfaces() As Long
    Dim t As Two, u As Three, f As IFirst, s As ISecond, w As IThird, r As Long
    Set t = New Two
    t.Count = 4
    r = t.Own()
    Set f = t
    Set s = t
    r = r + f.FirstValue() + s.SecondValue() + Len(f.FirstName) + Len(s.SecondName)
    Set u = New Three
    r = r + u.Own()
    Set f = u
    Set s = u
    Set w = u
    r = r + f.FirstValue() + s.SecondValue() + w.ThirdValue()
    r = r + Len(f.FirstName) + Len(s.SecondName) + Len(w.ThirdName)
    Interfaces = r
End Function

Private Function Order() As Long
    Dim m As Mixed, f As IFirst
    Set m = New Mixed
    m.Value = 5
    m.Note = "five"
    Set m.Shown = New Feeder
    m.Last
    Set f = m
    Order = m.First() + m.Inner() + f.FirstValue() + Len(f.FirstName)
End Function

Sub Main()
    Dim n As Double
    n = Members() + Interfaces() + Order()
End Sub
