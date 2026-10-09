Attribute VB_Name = "ByVals"
Option Explicit

' Variants pushed by value from every storage class, array element
' stores of literals and results, Currency against Double, Variant
' comparisons as conditions.

Private mV As Variant, mR As Rec

Private Function PV(ByVal v As Variant) As Long
    PV = VarType(v)
End Function

Private Function Txt() As String
    Txt = "t"
End Function

Public Function Pushes(rv As Variant, av() As Variant, rr As Rec) As Long
    Dim lv As Variant, k As Long, sa(2) As String, ia(2) As Integer, ca(2) As Currency
    k = PV(mV) + PV(gV) + PV(av(1)) + PV(rr.fV) + PV(mR.fV) + PV(gR.fV) + PV(rv) + PV(lv)
    sa(1) = "lit": sa(2) = Txt(): ia(1) = 5: ca(1) = 2.5@
    Pushes = k
End Function

Public Function Mixed(ByVal c As Currency, ByVal d As Double, ByVal v As Variant, ByVal w As Variant, ByVal s As Single, ByVal i As Integer) As Long
    Dim k As Long
    If c = d Then k = 1
    If c <> d Then k = 2
    If c < d Then k = 3
    If c <= d Then k = 4
    If c > d Then k = 5
    If c >= d Then k = 6
    If v = w Then k = 7
    If v <> w Then k = 8
    If v < w Then k = 9
    If v <= w Then k = 10
    If v > w Then k = 11
    If v >= w Then k = 12
    If v Like w Then k = 13
    s = CSng(s): i = CInt(v): c = CCur(v): k = k + CByte(v) + LenB(v) + InStr(1, v, w) + InStrB(1, v, w) + StrComp(v, w, 1)
    Mixed = k
End Function

