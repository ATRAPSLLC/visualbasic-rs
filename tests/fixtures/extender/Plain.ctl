VERSION 5.00
Begin VB.UserControl Plain 
   ClientHeight    =   600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   1500
   ScaleHeight     =   600
   ScaleWidth      =   1500
End
Attribute VB_Name = "Plain"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' A UserControl that differs from Plain in one designer property.

Public Event Ping(ByVal N As Long)

Private m_N As Long

Public Property Get N() As Long
    N = m_N
End Property

Public Property Let N(ByVal v As Long)
    m_N = v
    RaiseEvent Ping(v)
End Property
