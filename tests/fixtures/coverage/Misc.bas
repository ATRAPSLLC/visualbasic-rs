Attribute VB_Name = "Misc"
Option Explicit

' File input of every type, printing forms, late binding on Variants,
' array statements, and statements that end or stop the program.

Public Sub Files(ByVal path As String)
    Dim b As Byte, i As Integer, l As Long, s As Single, d As Double, c As Currency, t As Date, st As String, v As Variant, f As Boolean, fx As String * 8, r As FileRec
    Open path For Output As #1
    Print #1, i; l, s; Spc(2); d; Tab(4); c
    Print #1,
    Print #1, st;
    Write #1, b, i, l, s, d, c, t, st, v, f
    Close #1
    Open path For Input As #1
    Input #1, b, i, l, s, d, c, t, st, v, f
    Line Input #1, st
    Close #1
    Open path For Random As #2 Len = 64
    Put #2, 1, fx
    Get #2, 1, fx
    Put #2, 2, r
    Get #2, 2, r
    Put #2, , fx
    Get #2, , fx
    Lock #2, 1
    Unlock #2, 1
    Close #2
    Name path As path & ".bak"
    Debug.Print i; st
    Debug.Assert i = 0
End Sub

Public Function Late(ByVal o As Object, v As Variant) As Variant
    Dim x As Variant
    x = v.Count
    v.Name = "x"
    Set v.Item = o
    x = v.Item(1)
    v.Add 1, "k"
    x = v(1)
    v(2) = 5
    Set x = o
    Set Late = o
End Function

Public Sub Arrays()
    Dim a() As Integer, v As Variant, s() As String, r() As Rec, o() As Object, fixed(2) As Currency
    ReDim a(5)
    ReDim Preserve a(10)
    ReDim v(3)
    ReDim Preserve v(4)
    ReDim s(2)
    ReDim r(2)
    ReDim Preserve r(3)
    ReDim o(1)
    Erase a
    Erase s
    Erase r
    Erase o
    Erase fixed
    a = a
    v = a
    Dim n As New Collection
    n.Add 1
    Dim h As New Holder
    Call h.Round
End Sub

Public Sub Ender(ByVal k As Long)
    If k = 1 Then Stop
    If k = 2 Then End
    If k = 3 Then Error 5
End Sub

