VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Measure"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' An interface: methods returning each scalar type, implemented by Ring and
' called through the interface's vtable.

Public Function Area() As Single
End Function

Public Function Perimeter() As Double
End Function

Public Function Cost() As Currency
End Function

Public Function Made() As Date
End Function

Public Function Sides() As Byte
End Function

Public Function Label() As String
End Function

Public Function Weight() As Variant
End Function

Public Property Get Valid() As Boolean
End Property

Public Sub Resize(ByVal factor As Single)
End Sub
