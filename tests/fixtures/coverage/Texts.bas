Attribute VB_Name = "Texts"
Option Explicit
Option Compare Text

' String and Variant comparisons under Option Compare Text.

Public Function TextOps(ByVal a As String, ByVal b As String, ByVal v As Variant, ByVal w As Variant) As Long
    Dim k As Long, ok As Boolean
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b) Or (a Like b)
    If ok Then k = 1
    If (v = w) Or (v <> w) Or (v < w) Or (v <= w) Or (v > w) Or (v >= w) Or (v Like w) Then k = k + 2
    k = k - ((v = w) + (v < w) + (v > w))
    Select Case a
        Case "a" To "m": k = k + 4
    End Select
    Select Case v
        Case "a" To "m": k = k + 8
    End Select
    k = k + StrComp(a, b) + InStr(a, b)
    TextOps = k
End Function

