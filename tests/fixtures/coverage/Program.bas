Attribute VB_Name = "Program"
Option Explicit

' Calls every routine once.

Sub Main()
    Dim r As Rec, h As Holder, d As Double
    Dim arrB() As Byte, xB As Byte
    ReDim arrB(3)
    xB = OpB(3, 3, xB, arrB, r)
    Dim arrI() As Integer, xI As Integer
    ReDim arrI(3)
    xI = OpI(300, 300, xI, arrI, r)
    Dim arrL() As Long, xL As Long
    ReDim arrL(3)
    xL = OpL(70000, 70000, xL, arrL, r)
    Dim arrS() As Single, xS As Single
    ReDim arrS(3)
    xS = OpS(1.5, 1.5, xS, arrS, r)
    Dim arrD() As Double, xD As Double
    ReDim arrD(3)
    xD = OpD(2.25, 2.25, xD, arrD, r)
    Dim arrC() As Currency, xC As Currency
    ReDim arrC(3)
    xC = OpC(3.5@, 3.5@, xC, arrC, r)
    Dim arrT() As Date, xT As Date
    ReDim arrT(3)
    xT = OpT(#1/2/2003#, #1/2/2003#, xT, arrT, r)
    Dim arrStr() As String, xStr As String
    ReDim arrStr(3)
    xStr = OpStr("ab", "ab", xStr, arrStr, r)
    Dim arrV() As Variant, xV As Variant
    ReDim arrV(3)
    xV = OpV(7, 7, xV, arrV, r)
    Dim arrF() As Boolean, xF As Boolean
    ReDim arrF(3)
    xF = OpF(True, True, xF, arrF, r)
    d = Convert(1, 2, 3, 4, 5, 6, Now, "7", 8, True)
    d = d + TextOps("a", "b", "c", "d")
    Set h = New Holder
    d = d + h.Round()
    Files "x.txt"
    Arrays
    Ender 0
End Sub

