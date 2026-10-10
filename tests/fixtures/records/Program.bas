Attribute VB_Name = "Program"
Option Explicit

' Records the runtime reads through descriptors: Get and Put of a record
' with nested records, fixed and dynamic arrays, Variants and a fixed-length
' string; a fixed array inside a record; record arrays assigned, returned
' and erased; and For Each over a collection into a typed object variable.

Private Type Inner
    S As String
    N As Integer
End Type

Private Type Outer
    A As Long
    In1 As Inner
    Ins(2) As Inner
    Strs(3) As String
    V As Variant
    Dyn() As Long
    DynS() As String
    DynIn() As Inner
    Grid(1 To 2, 3 To 5) As Integer
    Fx As String * 5
    B As Byte
End Type

Private Type Plain
    X As Long
    Y As Double
End Type

Private m_Fixed(3) As Outer
Private m_Plain(2) As Plain

Private Function MakeArr() As Outer()
    Dim t() As Outer
    ReDim t(1)
    MakeArr = t
End Function

Private Sub Touch(ByRef n As Integer)
    n = n + 1
End Sub

Public Sub Main()
    Dim o As Outer, p As Plain, f As Integer
    Dim a() As Outer, b() As Outer, loc(2) As Outer
    Dim c As Collection, it As Item
    o.Grid(2, 4) = 7
    Touch o.Grid(1, 5)
    f = FreeFile
    Open "x.bin" For Random As #f Len = 400
    Put #f, , o
    Get #f, 1, o
    Put #f, , p
    Get #f, , p
    Close #f
    ReDim a(2)
    b = a
    b = MakeArr()
    Erase loc
    Erase m_Fixed
    Erase m_Plain
    Erase a
    Set c = New Collection
    For Each it In c
        Debug.Print it.Name
    Next
End Sub
