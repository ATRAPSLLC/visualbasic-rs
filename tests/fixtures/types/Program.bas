Attribute VB_Name = "Program"
Option Explicit

' Calls every Kinds method early-bound (VCallHresult, argument widths per
' type) and one module procedure per return type (the ImpAdCall* forms).

Public Type Pair
    a As Long
    b As String
End Type

Private Declare Sub Sleep Lib "kernel32" (ByVal ms As Long)
Private Declare Function MulDiv Lib "kernel32" (ByVal a As Long, ByVal b As Long, ByVal c As Long) As Long
Private Declare Function lstrcmpA Lib "kernel32" (ByVal a As String, ByVal b As String) As Long

Sub Main()
    Dim k As Kinds, o As Object, p As Pair
    Dim bo As Boolean, da As Date, it As Integer, lo As Long, si As Single
    Dim db As Double, cu As Currency, st As String, va As Variant, by As Byte
    Dim al() As Long, sa() As String, vv() As Variant, ra() As Long
    Set k = New Kinds
    bo = k.B(True)
    da = k.D(#1/2/2003#)
    it = k.I(3)
    lo = k.L(4)
    si = k.S(1.5)
    db = k.R(2.25)
    cu = k.C(3.5@)
    st = k.T("s")
    va = k.V(7)
    Set o = k.O(k)
    by = k.Y(9)
    Set k = k.K(k)
    lo = k.Refs(bo, da, it, lo, si, db, cu, st, va, o, by)
    ReDim al(2): ReDim sa(1): ReDim vv(1)
    ra = k.Arrays(al, sa, vv)
    lo = k.Opts()
    lo = k.Opts(1, "x", 2.5, False, 3)
    k.Many 1, 2, "three", 4#
    k.Item(1, 2) = "v"
    st = k.Item(1, 2)
    p.a = 1: p.b = "pair"
    lo = k.Rec(p)
    cu = k.UseHidden()
    k.Raise
    bo = MB(bo): da = MD(da): it = MI(it): lo = ML(lo): si = MS(si)
    db = MR(db): cu = MC(cu): st = MT(st): va = MV(va): by = MY(by)
    Set o = MO(o)
    Sleep 0
    lo = MulDiv(lo, 2, 3)
    lo = lstrcmpA(st, "a")
End Sub

Public Function MB(ByVal x As Boolean) As Boolean
    MB = x
End Function

Public Function MD(ByVal x As Date) As Date
    MD = x
End Function

Public Function MI(ByVal x As Integer) As Integer
    MI = x
End Function

Public Function ML(ByVal x As Long) As Long
    ML = x
End Function

Public Function MS(ByVal x As Single) As Single
    MS = x
End Function

Public Function MR(ByVal x As Double) As Double
    MR = x
End Function

Public Function MC(ByVal x As Currency) As Currency
    MC = x
End Function

Public Function MT(ByVal x As String) As String
    MT = x
End Function

Public Function MV(ByVal x As Variant) As Variant
    MV = x
End Function

Public Function MY(ByVal x As Byte) As Byte
    MY = x
End Function

Public Function MO(ByVal x As Object) As Object
    Set MO = x
End Function
