VERSION 5.00
Begin VB.Form Child 
   Caption         =   "Child"
   ClientHeight    =   3090
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   4680
   LinkTopic       =   "Child"
   MDIChild        =   -1  'True
   ScaleHeight     =   3090
   ScaleWidth      =   4680
   Begin VB.CommandButton Command1 
      Height          =   375
      Index           =   0
      Left            =   120
      TabIndex        =   0
      Caption         =   "A"
      Top             =   120
      Width           =   1215
   End
   Begin VB.CommandButton Command1 
      Height          =   375
      Index           =   1
      Left            =   120
      TabIndex        =   1
      Caption         =   "B"
      Top             =   600
      Width           =   1215
   End
   Begin VB.TextBox Text1 
      Height          =   375
      Index           =   0
      Left            =   1560
      TabIndex        =   2
      Text            =   "0"
      Top             =   120
      Width           =   1215
   End
   Begin VB.TextBox Text1 
      Height          =   375
      Index           =   1
      Left            =   1560
      TabIndex        =   3
      Text            =   "1"
      Top             =   600
      Width           =   1215
   End
   Begin VB.Label Label1 
      Height          =   375
      Index           =   0
      Left            =   120
      TabIndex        =   4
      Caption         =   "x"
      Top             =   1200
      Width           =   1215
   End
   Begin VB.Label Label1 
      Height          =   375
      Index           =   1
      Left            =   120
      TabIndex        =   5
      Caption         =   "y"
      Top             =   1680
      Width           =   1215
   End
End
Attribute VB_Name = "Child"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' An MDI child form with three control arrays (CommandButton, TextBox,
' Label), each walked with For Each and its members called; built-in
' methods with optional arguments left out (Move, Show, Refresh).

Private m_Total As Long

Public Sub Reset()
    m_Total = 0
    Label1(0).Caption = "reset"
End Sub

Public Property Get Total() As Long
    Total = m_Total + Text1.Count + Label1.UBound + Command1.LBound
End Property

Private Function Walk() As Long
    Dim c As Object, n As Long
    For Each c In Command1
        n = n + c.Index
    Next
    For Each c In Text1
        n = n + Len(c.Text)
    Next
    For Each c In Label1
        n = n + Len(c.Caption)
    Next
    Walk = n
End Function

Private Sub Command1_Click(Index As Integer)
    m_Total = m_Total + Index + Walk()
    Text1(Index).Text = CStr(m_Total)
    Me.Move 0
    Me.Move 0, 0, 3000
    Me.Refresh
End Sub

Private Sub Text1_Change(Index As Integer)
    Label1(Index).Caption = Text1(Index).Text
End Sub

Private Sub Label1_Click(Index As Integer)
    m_Total = m_Total - Index
End Sub

Private Sub Form_Load()
    m_Total = Walk()
End Sub
