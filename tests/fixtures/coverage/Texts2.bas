Attribute VB_Name = "Texts2"
Option Explicit
Option Compare Text

' Variant comparisons as conditions under Option Compare Text.

Public Function TextConds(ByVal v As Variant, ByVal w As Variant) As Long
    Dim k As Long
    If v = w Then k = 1
    If v <> w Then k = 2
    If v < w Then k = 3
    If v <= w Then k = 4
    If v > w Then k = 5
    If v >= w Then k = 6
    If v Like w Then k = 7
    TextConds = k
End Function

