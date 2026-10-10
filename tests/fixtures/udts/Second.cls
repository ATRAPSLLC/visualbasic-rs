VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Second"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' A Type with the same members as Shapes' PrivT, and a parameter of
' another class's Type.

Public Type Pair
    K As String
    V As Long
End Type

Public Function Mk(ByVal k As String) As Pair
    Mk.K = k
End Function

Public Function Pt(p As Point) As Long
    Pt = p.Y
End Function
