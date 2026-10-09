VERSION 5.00
Begin VB.UserControl Panel 
   ClientHeight    =   1200
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   3000
   ScaleHeight     =   1200
   ScaleWidth      =   3000
   Begin OcxLib.Knob Knob1 
      Height          =   600
      Left            =   120
      TabIndex        =   0
      Top             =   120
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin OcxLib.Knob Knob2 
      Height          =   600
      Index           =   0
      Left            =   1680
      TabIndex        =   1
      Top             =   120
      Width           =   1200
      _ExtentX        =   2117
      _ExtentY        =   1058
   End
End
Attribute VB_Name = "Panel"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' A public UserControl hosting Knob: a handler for each of Knob's own
' events and for extender events, on a single Knob and on a Knob control
' array; calls on the hosted Knob's members.

Public Event Summary(ByVal Text As String)

Private m_Turns As Long

Private Sub Knob1_Turned(ByVal Position As Long, ByVal Delta As Double)
    m_Turns = m_Turns + 1
    RaiseEvent Summary("turned " & Position & " by " & Delta)
End Sub

Private Sub Knob1_Reset()
    m_Turns = 0
End Sub

Private Sub Knob1_GotFocus()
    Knob1.Position = Knob1.Position + 10
End Sub

Private Sub Knob2_Turned(Index As Integer, ByVal Position As Long, ByVal Delta As Double)
    m_Turns = m_Turns + Index
End Sub

Private Sub Knob2_Reset(Index As Integer)
    Knob2(Index).Zero
End Sub

Public Function Turns() As Long
    Turns = m_Turns + Knob2.Count
    Knob1.Zero
End Function
