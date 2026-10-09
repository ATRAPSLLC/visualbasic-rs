VERSION 5.00
Object = "{248DD890-BB45-11CF-9ABC-0080C7E7B78D}#1.0#0"; "MSWINSCK.OCX"
Object = "{48E59290-9880-11CF-9754-00AA00C00908}#1.0#0"; "MSINET.OCX"
Object = "{F9043C88-F6F2-101A-A3C9-08002B2F49FB}#1.2#0"; "COMDLG32.OCX"
Object = "{C932BA88-4374-101B-A56C-00AA003668DC}#1.1#0"; "MSMASK32.OCX"
Object = "{831FDD16-0C5C-11D2-A9FC-0000F8754DA1}#2.0#0"; "MSCOMCTL.OCX"
Begin VB.Form Client 
   Caption         =   "Client"
   ClientHeight    =   3090
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   4680
   LinkTopic       =   "Client"
   ScaleHeight     =   3090
   ScaleWidth      =   4680
   StartUpPosition =   3  'Windows Default
   Begin MSWinsockLib.Winsock Winsock1 
      Left            =   120
      Top             =   120
      _ExtentX        =   741
      _ExtentY        =   741
      _Version        =   393216
   End
   Begin MSWinsockLib.Winsock Peers 
      Index           =   0
      Left            =   600
      Top             =   120
      _ExtentX        =   741
      _ExtentY        =   741
      _Version        =   393216
   End
   Begin InetCtlsObjects.Inet Inet1 
      Left            =   1080
      Top             =   120
      _ExtentX        =   1005
      _ExtentY        =   1005
      _Version        =   393216
   End
   Begin MSMask.MaskEdBox Masked 
      Height          =   375
      Left            =   120
      TabIndex        =   0
      Top             =   720
      Width           =   2175
      _ExtentX        =   3836
      _ExtentY        =   661
      _Version        =   393216
      PromptChar      =   "_"
   End
   Begin MSComctlLib.ProgressBar ProgressBar1 
      Height          =   255
      Left            =   120
      TabIndex        =   1
      Top             =   1200
      Width           =   2175
      _ExtentX        =   3836
      _ExtentY        =   450
      _Version        =   393216
      Appearance      =   1
   End
   Begin MSComctlLib.ListView ListView1 
      Height          =   975
      Left            =   120
      TabIndex        =   2
      Top             =   1560
      Width           =   2175
      _ExtentX        =   3836
      _ExtentY        =   1720
      LabelWrap       =   -1  'True
      HideSelection   =   -1  'True
      _Version        =   393217
      ForeColor       =   -2147483640
      BackColor       =   -2147483643
      BorderStyle     =   1
      Appearance      =   1
      NumItems        =   0
   End
   Begin MSComctlLib.TreeView TreeView1 
      Height          =   975
      Left            =   2400
      TabIndex        =   3
      Top             =   1560
      Width           =   2175
      _ExtentX        =   3836
      _ExtentY        =   1720
      _Version        =   393217
      Style           =   7
      Appearance      =   1
   End
   Begin MSComDlg.CommonDialog CommonDialog1 
      Left            =   1680
      Top             =   120
      _ExtentX        =   847
      _ExtentY        =   847
      _Version        =   393216
   End
End
Attribute VB_Name = "Client"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' A form hosting third-party ActiveX controls: Winsock (one and a control
' array), Inet, CommonDialog, MaskEdBox (data-bindable properties) and the
' Common Controls (ProgressBar, ListView, TreeView). Handlers for their own events; calls on
' their methods and properties, with and without optional arguments.

Private m_Received As String

Public Sub Start(ByVal Host As String, ByVal Port As Long)
    Winsock1.RemoteHost = Host
    Winsock1.RemotePort = Port
    Winsock1.Connect
    Winsock1.Connect Host, Port
    Peers(0).LocalPort = Port + 1
    Peers(0).Listen
End Sub

Public Function Fetch(ByVal Url As String) As String
    Dim body As Variant
    body = Inet1.OpenURL(Url)
    body = Inet1.OpenURL(Url, 0)
    Inet1.Execute Url, "GET"
    Fetch = CStr(body)
End Function

Public Function PickFile() As String
    CommonDialog1.Filter = "All|*.*"
    CommonDialog1.ShowOpen
    PickFile = CommonDialog1.FileName
End Function

Private Sub Winsock1_Connect()
    Winsock1.SendData "hello"
End Sub

Private Sub Winsock1_DataArrival(ByVal bytesTotal As Long)
    Dim chunk As String
    Winsock1.GetData chunk, vbString
    Winsock1.GetData chunk, vbString, bytesTotal
    m_Received = m_Received & chunk
End Sub

Private Sub Winsock1_Error(ByVal Number As Integer, Description As String, ByVal Scode As Long, ByVal Source As String, ByVal HelpFile As String, ByVal HelpContext As Long, CancelDisplay As Boolean)
    CancelDisplay = True
    Winsock1.Close
End Sub

Private Sub Winsock1_GotFocus()
    m_Received = "focus"
End Sub

Private Sub Winsock1_Close()
    m_Received = ""
End Sub

Private Sub Peers_ConnectionRequest(Index As Integer, ByVal requestID As Long)
    Load Peers(Index + 1)
    Peers(Index + 1).Accept requestID
End Sub

Private Sub Peers_DataArrival(Index As Integer, ByVal bytesTotal As Long)
    Dim s As String
    Peers(Index).GetData s
    Peers(Index).SendData s
End Sub

Private Sub Inet1_StateChanged(ByVal State As Integer)
    If State = 12 Then m_Received = Inet1.GetChunk(1024)
End Sub

Private Sub Fill()
    Dim item As Object, node As Object
    Set item = ListView1.ListItems.Add(, "k1", "first")
    Set node = TreeView1.Nodes.Add(, , "root", "Root")
    Set node = TreeView1.Nodes.Add("root", 4, "leaf", "Leaf")
    ProgressBar1.Value = ListView1.ListItems.Count + TreeView1.Nodes.Count
End Sub

Private Sub ListView1_ItemClick(ByVal Item As MSComctlLib.ListItem)
    m_Received = Item.Text
End Sub

Private Sub TreeView1_NodeClick(ByVal Node As MSComctlLib.Node)
    m_Received = Node.Key
End Sub

Private Sub ProgressBar1_Click()
    ProgressBar1.Value = 0
    Fill
End Sub

Private Sub Masked_Change()
    m_Received = Masked.Text
End Sub

Private Sub Form_Load()
    m_Received = ""
    Winsock1.Protocol = 0
End Sub
