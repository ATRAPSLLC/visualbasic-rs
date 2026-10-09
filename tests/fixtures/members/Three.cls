VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Three"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Implements three interfaces.

Implements IFirst
Implements ISecond
Implements IThird

Public Function Own() As Long
    Own = 3
End Function

Private Function IFirst_FirstValue() As Long
    IFirst_FirstValue = 10
End Function

Private Property Get IFirst_FirstName() As String
    IFirst_FirstName = "one"
End Property

Private Function ISecond_SecondValue() As Long
    ISecond_SecondValue = 20
End Function

Private Property Get ISecond_SecondName() As String
    ISecond_SecondName = "two"
End Property

Private Function IThird_ThirdValue() As Long
    IThird_ThirdValue = 30
End Function

Private Property Get IThird_ThirdName() As String
    IThird_ThirdName = "three"
End Property
