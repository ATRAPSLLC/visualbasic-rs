VERSION 5.00
Begin VB.UserControl Board 
   ClientHeight    =   3600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   4800
   ScaleHeight     =   3600
   ScaleWidth      =   4800
   Begin ExtenderLib.Plain Plain1 
      Height          =   600
      Left            =   120
      TabIndex        =   0
      Top             =   120
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.AlignBox AlignBox1 
      Height          =   600
      Left            =   120
      TabIndex        =   1
      Top             =   520
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.NoFocus NoFocus1 
      Height          =   600
      Left            =   120
      TabIndex        =   2
      Top             =   920
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.ButtonLike ButtonLike1 
      Height          =   600
      Left            =   120
      TabIndex        =   3
      Top             =   1320
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Ghost Ghost1 
      Height          =   600
      Left            =   120
      TabIndex        =   4
      Top             =   1720
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Holder Holder1 
      Height          =   600
      Left            =   120
      TabIndex        =   5
      Top             =   2120
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Forwarder Forwarder1 
      Height          =   600
      Left            =   120
      TabIndex        =   6
      Top             =   2520
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Bound Bound1 
      Height          =   600
      Left            =   120
      TabIndex        =   7
      Top             =   2920
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Light Light1 
      Height          =   600
      Left            =   2000
      TabIndex        =   9
      Top             =   900
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin ExtenderLib.Coords Coords1 
      Height          =   600
      Left            =   2000
      TabIndex        =   8
      Top             =   120
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
End
Attribute VB_Name = "Board"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' Hosts one instance of each UserControl variant, handling its event.

Private m_Total As Long

Private Sub Plain1_Ping(ByVal N As Long)
    m_Total = m_Total + N
End Sub

Private Sub Ghost1_Ping(ByVal N As Long)
    m_Total = m_Total - N
End Sub

Private Sub Coords1_Pixels(ByVal XPos As Single, ByVal YPos As Single, ByVal XSize As Single, ByVal YSize As Single)
    m_Total = m_Total + XPos + YPos + XSize + YSize
End Sub

Private Sub Coords1_Himetric(ByVal XPos As Single, ByVal YPos As Single, ByVal XSize As Single, ByVal YSize As Single)
    m_Total = m_Total + XPos + YPos + XSize + YSize
End Sub

Private Sub Coords1_Container(ByVal XPos As OLE_XPOS_CONTAINER, ByVal YPos As OLE_YPOS_CONTAINER, ByVal XSize As OLE_XSIZE_CONTAINER, ByVal YSize As OLE_YSIZE_CONTAINER)
    m_Total = m_Total + XPos + YPos + XSize + YSize
End Sub

Private Sub Coords1_Referenced(XPos As OLE_XPOS_PIXELS, ByVal Plain As Long, YSize As OLE_YSIZE_HIMETRIC)
    m_Total = m_Total + XPos + Plain + YSize
End Sub

Private Sub Coords1_Others(ByVal Cancel As OLE_CANCELBOOL, ByVal Exclusive As OLE_OPTEXCLUSIVE, ByVal Default As OLE_ENABLEDEFAULTBOOL, ByVal Color As OLE_COLOR, ByVal Handle As OLE_HANDLE, ByVal Tri As OLE_TRISTATE)
    m_Total = m_Total + Color + Handle + Tri
End Sub

Private Sub Coords1_Variants(ByVal VV As Variant, VR As Variant, ByVal O As Object, ByVal C As Currency, ByVal D As Date)
    m_Total = m_Total + VV + C
End Sub

Public Function Total() As Long
    Total = m_Total + Bound1.N + AlignBox1.N
End Function
