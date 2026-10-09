Attribute VB_Name = "Globals"
Option Explicit

' Public variables of every type, reached from the other modules.

Public Type Rec
    fB As Byte
    fI As Integer
    fL As Long
    fS As Single
    fD As Double
    fC As Currency
    fT As Date
    fStr As String
    fV As Variant
    fF As Boolean
    fO As Object
    fFx As String * 8
End Type

Public Type FileRec
    n As Long
    x As Double
    c As Currency
    fx As String * 8
End Type

Public gB As Byte
Public gI As Integer
Public gL As Long
Public gS As Single
Public gD As Double
Public gC As Currency
Public gT As Date
Public gStr As String
Public gV As Variant
Public gF As Boolean
Public gO As Object
Public gR As Rec
Public gA(3) As Long

