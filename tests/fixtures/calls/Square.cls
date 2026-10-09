VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Square"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

Implements Shape

Private m_Side As Long

Public Sub Init(ByVal side As Long)
    m_Side = side
End Sub

Private Function Shape_Area() As Long
    Shape_Area = m_Side * m_Side
End Function

Private Property Get Shape_Name() As String
    Shape_Name = "square"
End Property
